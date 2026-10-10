//! 工具层 —— 每个 `#[derive(App)]` 结构体就是一个 MCP 工具 / CLI 子命令。
//!
//! # 两条硬规矩（都是踩过 lilyco 宏的坑之后定的）
//!
//! ## 1. 安全开关必须「正向 opt-in」
//!
//! `#[derive(App)]` 的 `default` **只写进 schema**（供 CLI 填 `default_value`）。
//! MCP 的 `tools/call` 走 `from_args`，缺参时的兜底是 `false` / `0.0` / `""`。
//! 所以 `dry_run: bool`（声明默认 true）在 MCP 下省略就变成 `false` —— **真的开始按键**。
//! 因此这里一律用 `apply: bool`：缺省即 false = 只决策不执行。
//!
//! ## 2. 可选标量一律 `Option<T>`
//!
//! 同理，`max_tokens: u32` 缺省会变成 `0`（不是声明的 512），送到 LLM 就是 400。
//! 改成 `Option<u32>` 后，缺省是 `None`，在代码里 `unwrap_or(默认值)` —— 语义明确。

use std::path::PathBuf;

use lgame_core::action::{Action, Verb};
use lgame_core::embodiment::Embodiment;
use lgame_core::error::GameError;
use lgame_core::policy::{self, LlmPolicy, Policy, ReplayPolicy};
use lgame_core::session::{self, Control, SessionConfig};
// `Driver` 必须显式导入：`observe` / `execute` 是 trait 方法，
// 只导入 `ScreenDriver` 只能拿到类型、拿不到方法（E0599）。
use lgame_core::{ccswitch, targets, Driver, ScreenDriver};
use lilyco::prelude::*;

use crate::convert::IntoLgame;

// ── 共用解析 ────────────────────────────────────────────────

/// 默认画面目录
const DEFAULT_OUT: &str = "lgame-captures";
/// 默认送 LLM 前的最大宽度（省 token）
const DEFAULT_MAX_WIDTH: u32 = 1280;

fn out_dir(raw: &Option<String>) -> PathBuf {
    let s = raw.as_deref().unwrap_or("").trim();
    if s.is_empty() {
        PathBuf::from(DEFAULT_OUT)
    } else {
        PathBuf::from(s)
    }
}

fn max_width(raw: Option<u32>) -> u32 {
    match raw {
        Some(w) if w >= 64 => w,
        _ => DEFAULT_MAX_WIDTH,
    }
}

/// 定位窗口：`window` 优先，其次 `target`（内置 id 或直接当窗口串用）
pub fn resolve_window(
    target: &Option<String>,
    window: &Option<String>,
) -> Result<String, GameError> {
    if let Some(w) = window.as_deref().map(str::trim).filter(|w| !w.is_empty()) {
        return Ok(w.to_string());
    }
    if let Some(t) = target.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        if let Some(tg) = targets::find(t) {
            return Ok(tg.window);
        }
        // 不是内置 id —— 当窗口标题串直接用（让用户不必先登记）
        return Ok(t.to_string());
    }
    Err(GameError::Config(
        "必须给 `target`（内置目标 id，如 minecraft / aic）或 `window`（窗口标题匹配串）。\
         用 `targets` 工具看内置目标，`doctor` 工具看当前可见窗口。"
            .into(),
    ))
}

/// 解析躯体档位：`auto` 时优先用目标自带的档位
pub fn resolve_embodiment(target: &Option<String>, spec: &str) -> Result<Embodiment, GameError> {
    let want_default = spec.trim().is_empty() || spec.trim().eq_ignore_ascii_case("auto");
    if want_default {
        if let Some(tg) = target
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .and_then(targets::find)
        {
            if let Some(e) = tg.embodiment() {
                return Ok(e);
            }
        }
    }
    lgame_core::embodiment::resolve(spec)
}

/// 解析逗号/空格分隔的动词串
///
/// 返回 `(认出来的, 没认出来的)` —— 没认出来的不报错，
/// 但会被记进结果里，避免「写了 crouch 却没蹲下」变成玄学。
pub fn parse_verbs(s: &str) -> (Vec<Verb>, Vec<String>) {
    let mut ok = Vec::new();
    let mut bad = Vec::new();
    for tok in s
        .split([',', ' ', ';', '|'])
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        match Verb::parse(tok) {
            Some(v) => {
                if !ok.contains(&v) {
                    ok.push(v);
                }
            }
            None => bad.push(tok.to_string()),
        }
    }
    (ok, bad)
}

/// 从 CLI/MCP 参数拼一个 Action
#[allow(clippy::too_many_arguments)]
fn build_action(
    forward: Option<f64>,
    strafe: Option<f64>,
    yaw: Option<f64>,
    pitch: Option<f64>,
    duration_ms: Option<u64>,
    hold: &str,
    tap: &str,
    say: &Option<String>,
    note: &Option<String>,
) -> (Action, Vec<String>) {
    let (hold_v, mut bad) = parse_verbs(hold);
    let (tap_v, bad2) = parse_verbs(tap);
    bad.extend(bad2);

    let action = Action {
        forward: forward.unwrap_or(0.0) as f32,
        strafe: strafe.unwrap_or(0.0) as f32,
        yaw: yaw.unwrap_or(0.0) as f32,
        pitch: pitch.unwrap_or(0.0) as f32,
        duration_ms: duration_ms.unwrap_or(300),
        hold: hold_v,
        tap: tap_v,
        say: say.clone().filter(|s| !s.is_empty()),
        note: note.clone().filter(|s| !s.is_empty()),
    }
    .clamped();

    (action, bad)
}

// ── observe ─────────────────────────────────────────────────

/// 抓一帧画面
#[derive(App)]
#[app(
    name = "observe",
    run = "run_observe",
    about = "Capture one frame of a game window and save it as PNG. Read-only (safety T0). Returns { window, app, frame, width, height, tick, embodiment }. Use `doctor` first if you do not know which windows are visible. `target` is a built-in target id (minecraft / zombie-raid / aic / stronghold / tsukishita); `window` is a case-insensitive substring of the window title and wins over `target`."
)]
pub struct Observe {
    /// 内置目标 id（minecraft / aic / ...）
    #[arg(about = "Built-in target id (see the `targets` tool)")]
    target: Option<String>,
    /// 窗口标题匹配串（不区分大小写的子串）
    #[arg(about = "Case-insensitive substring of the window title; wins over `target`")]
    window: Option<String>,
    /// 躯体档位：3d / 2d / robot / auto
    #[arg(about = "Embodiment profile: 3d / 2d / robot / auto")]
    embodiment: Option<String>,
    /// 画面落盘目录
    #[arg(about = "Directory to write the PNG into")]
    out: Option<String>,
    /// 缩放后的最大宽度
    #[arg(about = "Downscale the frame to at most this width before saving")]
    max_width: Option<u32>,
}

pub fn run_observe(app: &Observe, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let window = resolve_window(&app.target, &app.window).lg()?;
    let em = resolve_embodiment(&app.target, app.embodiment.as_deref().unwrap_or("auto")).lg()?;
    let dir = out_dir(&app.out);
    let mw = max_width(app.max_width);

    let mut driver = ScreenDriver::new(&window, em.clone(), Some(mw)).lg()?;

    ctx.emit(Progress::Started {
        total: Some(1),
        message: Some(format!("抓取窗口 `{window}`")),
    });

    let obs = driver.observe(&dir).lg()?;
    let info = lgame_core::capture::list_windows().unwrap_or_default();
    let matched = info
        .into_iter()
        .find(|w| lgame_core::util::title_matches(&window, &w.title));

    let result = serde_json::json!({
        "window": obs.target,
        "frame": obs.frame.as_ref().map(|p| p.display().to_string()),
        "tick": obs.tick,
        "embodiment": em.id,
        "max_width": mw,
        "app": matched.as_ref().map(|w| w.app.clone()),
        "width": matched.as_ref().map(|w| w.width),
        "height": matched.as_ref().map(|w| w.height),
        "minimized": matched.as_ref().map(|w| w.minimized),
    });

    ctx.done(result.clone(), 0);
    Ok(result)
}

// ── act ─────────────────────────────────────────────────────

/// 执行一个动作
#[derive(App)]
#[app(
    name = "act",
    run = "run_act",
    safety = "t1",
    about = "Execute ONE action on a game window (keyboard/mouse injection). DRY RUN BY DEFAULT: pass `apply: true` to actually inject input (safety tier T1 — the MCP surface allows it, so prefer dry-run first). Action fields: `forward`/`strafe` in -1..1 (+1 = forward / right), `yaw`/`pitch` in degrees (+ = turn right / look up), `duration-ms` how long to hold, `hold` and `tap` are comma-separated verb names held or tapped once (jump, crouch, sprint, walk, interact, attack, alt_attack, reload, dodge, aim, inventory, map, pause, confirm, cancel, chat), `say` types text into chat. Returns { dry_run, action, summary, warnings, window, embodiment }."
)]
pub struct Act {
    #[arg(about = "Built-in target id (see the `targets` tool)")]
    target: Option<String>,
    #[arg(about = "Case-insensitive substring of the window title; wins over `target`")]
    window: Option<String>,
    #[arg(about = "Embodiment profile: 3d / 2d / robot / auto")]
    embodiment: Option<String>,
    /// 前后：+1 向前
    #[arg(about = "Forward/backward, -1..1 (+1 = forward)")]
    forward: Option<f64>,
    /// 左右：+1 向右
    #[arg(about = "Strafe left/right, -1..1 (+1 = right)")]
    strafe: Option<f64>,
    /// 偏航：+ 向右转
    #[arg(about = "Yaw in degrees (+ = turn right); ignored by embodiments without a look axis")]
    yaw: Option<f64>,
    /// 俯仰：+ 抬头
    #[arg(about = "Pitch in degrees (+ = look up); ignored by embodiments without a look axis")]
    pitch: Option<f64>,
    #[arg(about = "How long to hold the action, in milliseconds")]
    duration_ms: Option<u64>,
    #[arg(about = "Comma-separated verbs to HOLD during the action, e.g. 'sprint,aim'")]
    hold: Option<String>,
    #[arg(about = "Comma-separated verbs to TAP once, e.g. 'jump'")]
    tap: Option<String>,
    #[arg(about = "Text to type into the in-game chat")]
    say: Option<String>,
    #[arg(about = "One-line reason for this action (recorded, not executed)")]
    note: Option<String>,
    /// 真的注入输入（默认 false = 试跑）
    #[arg(
        about = "Actually inject the input. Default false = dry run (only reports what it would do)"
    )]
    apply: bool,
}

pub fn run_act(app: &Act, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let window = resolve_window(&app.target, &app.window).lg()?;
    let em = resolve_embodiment(&app.target, app.embodiment.as_deref().unwrap_or("auto")).lg()?;

    let (action, unknown) = build_action(
        app.forward,
        app.strafe,
        app.yaw,
        app.pitch,
        app.duration_ms,
        app.hold.as_deref().unwrap_or(""),
        app.tap.as_deref().unwrap_or(""),
        &app.say,
        &app.note,
    );

    if !unknown.is_empty() {
        ctx.log(
            LogLevel::Warn,
            format!("未知动词已忽略: {}", unknown.join(", ")),
        );
    }

    if !app.apply {
        let result = serde_json::json!({
            "dry_run": true,
            "window": window,
            "embodiment": em.id,
            "action": action,
            "summary": action.summary(),
            "unknown_verbs": unknown,
            "hint": "这是试跑。要真的按键，传 apply=true",
        });
        ctx.done(result.clone(), 0);
        return Ok(result);
    }

    let mut driver = ScreenDriver::new(&window, em.clone(), None).lg()?;
    ctx.emit(Progress::Started {
        total: Some(1),
        message: Some(format!("执行 {}", action.summary())),
    });

    let warnings = driver.execute(&action).lg()?;
    for w in &warnings {
        ctx.log(LogLevel::Warn, w.clone());
    }

    let result = serde_json::json!({
        "dry_run": false,
        "window": window,
        "embodiment": em.id,
        "action": action,
        "summary": action.summary(),
        "warnings": warnings,
        "unknown_verbs": unknown,
    });
    ctx.done(result.clone(), 0);
    Ok(result)
}

// ── step / run ──────────────────────────────────────────────

/// `step` 与 `run` 共用的驱动参数
struct DriveSpec {
    window: String,
    embodiment: Embodiment,
    goal: String,
    steps: u32,
    step_delay_ms: u64,
    out: PathBuf,
    apply: bool,
    vision: bool,
    max_tokens: u32,
    max_width: u32,
    replay: bool,
}

fn drive(ctx: &Context, spec: DriveSpec) -> Result<serde_json::Value, AppError> {
    if spec.goal.trim().is_empty() {
        return Err(AppError::InvalidArg(
            "必须给 `goal`（自然语言目标），例如 goal=\"走到村子入口\"".into(),
        ));
    }

    let mut driver =
        ScreenDriver::new(&spec.window, spec.embodiment.clone(), Some(spec.max_width)).lg()?;

    let mut policy: Box<dyn Policy> = if spec.replay {
        Box::new(ReplayPolicy::looping_default())
    } else {
        let endpoint = ccswitch::discover().lg()?;
        ctx.log(
            LogLevel::Info,
            format!(
                "LLM 端点 {}（来源 {}，可达 {}）",
                endpoint.base_url, endpoint.source, endpoint.reachable
            ),
        );
        Box::new(LlmPolicy::new(endpoint, spec.max_tokens, spec.vision))
    };

    let cfg = SessionConfig {
        goal: spec.goal.clone(),
        max_steps: spec.steps,
        step_delay_ms: spec.step_delay_ms,
        out_dir: spec.out.clone(),
        dry_run: !spec.apply,
    };

    let control = Control::new();
    let ctl_in_cb = control.clone();

    ctx.emit(Progress::Started {
        total: Some(spec.steps as u64),
        message: Some(format!(
            "目标：{}｜策略：{}｜{}",
            spec.goal,
            policy.name(),
            if spec.apply { "执行" } else { "试跑" }
        )),
    });

    let outcome = session::run(
        &mut driver,
        &mut policy,
        &cfg,
        &control,
        &mut |step, rec| {
            ctx.tick(step as u64, Some(spec.steps as u64), rec.summary());
            if ctx.is_cancelled() {
                ctl_in_cb.stop();
            }
        },
    )
    .lg()?;

    let mut result = outcome.to_json();
    if let Some(obj) = result.as_object_mut() {
        obj.insert("goal".into(), serde_json::json!(spec.goal));
        obj.insert("window".into(), serde_json::json!(spec.window));
        obj.insert("embodiment".into(), serde_json::json!(spec.embodiment.id));
        obj.insert("policy".into(), serde_json::json!(policy.name()));
        obj.insert("dry_run".into(), serde_json::json!(!spec.apply));
    }

    ctx.done(result.clone(), 0);
    Ok(result)
}

/// 走一步：观察 → 决策 → （可选）执行
#[derive(App)]
#[app(
    name = "step",
    run = "run_step",
    safety = "t1",
    about = "Run ONE perceive-decide-act step against a game window: capture a frame, ask the local LLM (cc-switch) what to do given `goal`, then optionally inject it. DRY RUN BY DEFAULT: pass `apply: true` to actually inject input. `replay: true` uses an offline scripted policy instead of the LLM — useful to verify the pipeline without cc-switch running. Returns { steps, stopped_because, trace, last_actions, goal, window, embodiment, policy, dry_run }."
)]
pub struct Step {
    #[arg(about = "Built-in target id (see the `targets` tool)")]
    target: Option<String>,
    #[arg(about = "Case-insensitive substring of the window title; wins over `target`")]
    window: Option<String>,
    /// 自然语言目标
    #[arg(about = "Natural-language goal, e.g. 'walk to the village entrance'")]
    goal: String,
    #[arg(about = "Embodiment profile: 3d / 2d / robot / auto")]
    embodiment: Option<String>,
    #[arg(about = "Directory for frames and trace.jsonl")]
    out: Option<String>,
    #[arg(about = "Downscale frames to at most this width before sending to the LLM")]
    max_width: Option<u32>,
    #[arg(about = "Max tokens for the LLM reply")]
    max_tokens: Option<u32>,
    #[arg(
        about = "Send the frame to the LLM as an image (set no_vision=true for text-only models)"
    )]
    no_vision: bool,
    #[arg(about = "Use the offline scripted policy instead of the LLM")]
    replay: bool,
    #[arg(about = "Actually inject the input. Default false = dry run")]
    apply: bool,
}

pub fn run_step(app: &Step, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let window = resolve_window(&app.target, &app.window).lg()?;
    let em = resolve_embodiment(&app.target, app.embodiment.as_deref().unwrap_or("auto")).lg()?;
    drive(
        ctx,
        DriveSpec {
            window,
            embodiment: em,
            goal: app.goal.clone(),
            steps: 1,
            step_delay_ms: 0,
            out: out_dir(&app.out),
            apply: app.apply,
            vision: !app.no_vision,
            max_tokens: app.max_tokens.unwrap_or(512),
            max_width: max_width(app.max_width),
            replay: app.replay,
        },
    )
}

/// 连续走多步
#[derive(App)]
#[app(
    name = "run",
    run = "run_run",
    safety = "t1",
    about = "Run a perceive-decide-act LOOP against a game window for up to `steps` iterations. DRY RUN BY DEFAULT: pass `apply: true` to actually inject input. Each step: capture a frame, ask the local LLM (cc-switch) for one action given `goal` and the last 5 actions, then inject it. `replay: true` swaps in an offline scripted policy (no LLM, no cc-switch needed). A trace.jsonl is written into `out` so every step can be reviewed afterwards. Returns { steps, stopped_because, trace, last_actions, goal, window, embodiment, policy, dry_run }."
)]
pub struct Run {
    #[arg(about = "Built-in target id (see the `targets` tool)")]
    target: Option<String>,
    #[arg(about = "Case-insensitive substring of the window title; wins over `target`")]
    window: Option<String>,
    #[arg(about = "Natural-language goal, e.g. 'chop three trees'")]
    goal: String,
    #[arg(about = "Embodiment profile: 3d / 2d / robot / auto")]
    embodiment: Option<String>,
    #[arg(about = "Maximum number of perceive-decide-act iterations")]
    steps: Option<u32>,
    #[arg(about = "Milliseconds to wait between steps (let the game render)")]
    step_delay_ms: Option<u64>,
    #[arg(about = "Directory for frames and trace.jsonl")]
    out: Option<String>,
    #[arg(about = "Downscale frames to at most this width before sending to the LLM")]
    max_width: Option<u32>,
    #[arg(about = "Max tokens for each LLM reply")]
    max_tokens: Option<u32>,
    #[arg(about = "Send frames to the LLM as images (set no_vision=true for text-only models)")]
    no_vision: bool,
    #[arg(about = "Use the offline scripted policy instead of the LLM")]
    replay: bool,
    #[arg(about = "Actually inject the input. Default false = dry run")]
    apply: bool,
}

pub fn run_run(app: &Run, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let window = resolve_window(&app.target, &app.window).lg()?;
    let em = resolve_embodiment(&app.target, app.embodiment.as_deref().unwrap_or("auto")).lg()?;
    let steps = app.steps.unwrap_or(10).clamp(1, 10_000);
    drive(
        ctx,
        DriveSpec {
            window,
            embodiment: em,
            goal: app.goal.clone(),
            steps,
            step_delay_ms: app.step_delay_ms.unwrap_or(300),
            out: out_dir(&app.out),
            apply: app.apply,
            vision: !app.no_vision,
            max_tokens: app.max_tokens.unwrap_or(512),
            max_width: max_width(app.max_width),
            replay: app.replay,
        },
    )
}

// ── targets ─────────────────────────────────────────────────

/// 列出内置目标
#[derive(App)]
#[app(
    name = "targets",
    run = "run_targets",
    about = "List the built-in game targets (id, window title match, embodiment, why it was picked) plus the available embodiment profiles. Read-only (safety T0). Use this to discover a valid `target` value before calling observe/act/step/run."
)]
pub struct Targets {
    /// 只看某个目标的详情
    #[arg(about = "Show details for a single target id")]
    id: Option<String>,
}

pub fn run_targets(app: &Targets, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let all = targets::builtin();

    let list: Vec<serde_json::Value> = match app.id.as_deref().map(str::trim) {
        Some(id) if !id.is_empty() => {
            let t = targets::find(id).ok_or_else(|| {
                AppError::InvalidArg(format!("没有内置目标 `{id}`；不带 id 调用可看全部"))
            })?;
            vec![serde_json::json!({
                "id": t.id, "title": t.title, "window": t.window,
                "embodiment": t.embodiment, "note": t.note,
            })]
        }
        _ => all
            .iter()
            .map(|t| {
                serde_json::json!({
                    "id": t.id, "title": t.title, "window": t.window,
                    "embodiment": t.embodiment, "note": t.note,
                })
            })
            .collect(),
    };

    let embodiments: Vec<serde_json::Value> = Embodiment::all()
        .iter()
        .map(|e| {
            serde_json::json!({
                "id": e.id, "title": e.title,
                "look": e.look, "locomotion": e.locomotion,
                "verbs": e.verbs.iter().map(|v| v.as_str()).collect::<Vec<_>>(),
            })
        })
        .collect();

    let result = serde_json::json!({
        "targets": list,
        "embodiments": embodiments,
        "default_embodiment": lgame_core::embodiment::default_id(),
    });
    ctx.done(result.clone(), 0);
    Ok(result)
}

// ── doctor ──────────────────────────────────────────────────

/// 环境自检
#[derive(App)]
#[app(
    name = "doctor",
    run = "run_doctor",
    about = "Diagnose the environment: list visible windows (so you can pick a `window` match string), report whether the local LLM endpoint (cc-switch) was found and is reachable, show the screen-backend status, and show the built-in targets. Read-only (safety T0). Run this FIRST when something does not work."
)]
pub struct Doctor {
    #[arg(about = "Only list windows whose title contains this substring")]
    filter: Option<String>,
    #[arg(about = "Maximum number of windows to list")]
    limit: Option<u32>,
}

pub fn run_doctor(app: &Doctor, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let filter = app.filter.clone().unwrap_or_default();
    let limit = app.limit.unwrap_or(40).clamp(1, 500);

    let mut windows: Vec<serde_json::Value> = Vec::new();
    let mut windows_error: Option<String> = None;
    match lgame_core::capture::list_windows() {
        Ok(all) => {
            for w in all.into_iter().take(limit as usize) {
                if !filter.is_empty() && !lgame_core::util::title_matches(&filter, &w.title) {
                    continue;
                }
                windows.push(serde_json::json!({
                    "title": w.title, "app": w.app,
                    "width": w.width, "height": w.height,
                    "minimized": w.minimized,
                }));
            }
        }
        Err(e) => windows_error = Some(e.to_string()),
    }

    let (endpoint, endpoint_error) = match ccswitch::discover() {
        Ok(ep) => {
            let v = serde_json::json!({
                "base_url": ep.base_url,
                "model": ep.model,
                "source": ep.source,
                "reachable": ep.reachable,
                "is_local_proxy": ep.is_local_proxy(),
                "note": ep.note,
            });
            (Some(v), None)
        }
        Err(e) => (None, Some(e.to_string())),
    };

    let result = serde_json::json!({
        "lgame_core_version": lgame_core::VERSION,
        "platform": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "screen_backend": lgame_core::SCREEN_ENABLED,
        "default_embodiment": lgame_core::embodiment::default_id(),
        "windows": windows,
        "windows_error": windows_error,
        "llm_endpoint": endpoint,
        "llm_endpoint_error": endpoint_error,
        "builtin_targets": targets::builtin().iter().map(|t| t.id.clone()).collect::<Vec<_>>(),
    });
    ctx.done(result.clone(), 0);
    Ok(result)
}

// ── think（宿主采样桥） ──────────────────────────────────────

/// 用宿主（MCP 客户端）的 LLM 做一次决策
#[derive(App)]
#[app(
    name = "think",
    run = "run_think",
    safety = "t1",
    about = "Ask the HOST LLM to decide the next action, using MCP's reverse-sampling bridge (`sampling/createMessage`) instead of calling cc-switch. This works only when the MCP client declares the `sampling` capability (e.g. Claude Desktop); otherwise it returns a clear error and you should use `step`/`run` with cc-switch instead. The host prompt is TEXT-ONLY (no image), so prefer `step` when the frame matters. DRY RUN BY DEFAULT: pass `apply: true` to inject the decided action."
)]
pub struct Think {
    #[arg(about = "Built-in target id (see the `targets` tool)")]
    target: Option<String>,
    #[arg(about = "Case-insensitive substring of the window title; wins over `target`")]
    window: Option<String>,
    #[arg(about = "Natural-language goal")]
    goal: String,
    #[arg(about = "Embodiment profile: 3d / 2d / robot / auto")]
    embodiment: Option<String>,
    #[arg(about = "Max tokens for the host LLM reply")]
    max_tokens: Option<u32>,
    #[arg(about = "Actually inject the decided action. Default false = dry run")]
    apply: bool,
}

pub fn run_think(app: &Think, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let window = resolve_window(&app.target, &app.window).lg()?;
    let em = resolve_embodiment(&app.target, app.embodiment.as_deref().unwrap_or("auto")).lg()?;
    let max_tokens = app.max_tokens.unwrap_or(512);

    let mut driver = ScreenDriver::new(&window, em.clone(), None).lg()?;
    let obs = driver.observe(&out_dir(&None)).lg()?;

    let system = policy::system_prompt(&em, &app.goal);
    let prompt = format!(
        "{system}\n\n# 当前情况\n这是第 {} 次观察，窗口 `{}`。\
         你只能看到这段文字（本次没有画面）。请给出下一步动作 JSON。",
        obs.tick, obs.target
    );

    ctx.log(LogLevel::Info, "通过宿主采样桥请求决策…");
    let raw = ctx.sample(&prompt, max_tokens)?;
    let action = policy::parse_action(&raw).lg()?;

    let mut warnings: Vec<String> = Vec::new();
    if app.apply {
        warnings = driver.execute(&action).lg()?;
        for w in &warnings {
            ctx.log(LogLevel::Warn, w.clone());
        }
    } else {
        warnings.push("dry-run：只决策未执行".into());
    }

    let result = serde_json::json!({
        "dry_run": !app.apply,
        "window": window,
        "embodiment": em.id,
        "action": action,
        "summary": action.summary(),
        "warnings": warnings,
        "raw_reply": raw,
        "channel": "host-sampling",
    });
    ctx.done(result.clone(), 0);
    Ok(result)
}
