//! 策略 —— 中间层的第四块地基：**看**（[`crate::observation::Observation`]）
//! 之后决定**做什么**（[`crate::action::Action`]）。
//!
//! 两个实现：
//! - [`LlmPolicy`] 调本地 cc-switch 代理（Anthropic Messages 协议，支持视觉）
//! - [`ReplayPolicy`] 离线脚本回放 —— 不碰网络，用于 CI 与「先验证循环再谈智能」

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::action::Action;
use crate::ccswitch::Endpoint;
use crate::embodiment::{Embodiment, Locomotion, LookMode};
use crate::error::{GameError, Result};
use crate::observation::Observation;
use crate::trace::StepRecord;
use crate::util::{base64_encode, extract_json_object, strip_code_fence};

/// 决策时能看到的全部上下文
pub struct PolicyCtx<'a> {
    /// 高层目标（自然语言），如「走出这片树林」
    pub goal: &'a str,
    /// 当前躯体档位
    pub embodiment: &'a Embodiment,
    /// 本步的观察
    pub observation: &'a Observation,
    /// 已经做过的动作（策略可只用最近几步）
    pub history: &'a [StepRecord],
}

/// 决策器
pub trait Policy: Send {
    fn name(&self) -> &str;
    fn decide(&mut self, ctx: &PolicyCtx<'_>) -> Result<Action>;
}

/// 让 `Box<dyn Policy>` 也能当策略用 —— 运行期在 LLM / 回放之间切换需要它。
///
/// （泛型约束不会自动解引用，所以这个转发 impl 必须显式写。）
impl<T: Policy + ?Sized> Policy for Box<T> {
    fn name(&self) -> &str {
        (**self).name()
    }

    fn decide(&mut self, ctx: &PolicyCtx<'_>) -> Result<Action> {
        (**self).decide(ctx)
    }
}

// ── 离线脚本策略 ────────────────────────────────────────────

/// 按固定脚本循环输出动作 —— 不调 LLM。
///
/// 存在的意义：把「中间层的循环是否跑得通」与「LLM 是否可用」解耦。
/// CI 里跑这个，本机没启动 cc-switch 时也能验证观察→决策→执行→记录全链路。
#[derive(Debug, Clone, Default)]
pub struct ReplayPolicy {
    script: Vec<Action>,
    idx: usize,
}

impl ReplayPolicy {
    pub fn new(script: Vec<Action>) -> Self {
        ReplayPolicy { script, idx: 0 }
    }

    /// 从 JSON 数组加载脚本：`[{"forward":1.0,"duration_ms":300}, ...]`
    pub fn from_json(s: &str) -> Result<Self> {
        let script: Vec<Action> = serde_json::from_str(s.trim())
            .map_err(|e| GameError::Config(format!("回放脚本不是动作数组: {e}")))?;
        Ok(ReplayPolicy::new(script))
    }

    /// 默认三步循环：前进 → 右转 → 跳。用来证明链路通。
    pub fn looping_default() -> Self {
        ReplayPolicy::new(vec![
            Action {
                forward: 1.0,
                duration_ms: 300,
                note: Some("前进".into()),
                ..Default::default()
            },
            Action {
                yaw: 45.0,
                duration_ms: 250,
                note: Some("右转".into()),
                ..Default::default()
            },
            Action {
                tap: vec![crate::action::Verb::Jump],
                duration_ms: 150,
                note: Some("跳".into()),
                ..Default::default()
            },
        ])
    }

    pub fn len(&self) -> usize {
        self.script.len()
    }

    pub fn is_empty(&self) -> bool {
        self.script.is_empty()
    }
}

impl Policy for ReplayPolicy {
    fn name(&self) -> &str {
        "replay"
    }

    fn decide(&mut self, _ctx: &PolicyCtx<'_>) -> Result<Action> {
        if self.script.is_empty() {
            return Ok(Action::wait(200));
        }
        let a = self.script[self.idx % self.script.len()].clone();
        self.idx += 1;
        Ok(a.clamped())
    }
}

// ── LLM 策略 ────────────────────────────────────────────────

/// 调 Anthropic 兼容端点（本地 cc-switch 代理 / 任何兼容网关）
pub struct LlmPolicy {
    endpoint: Endpoint,
    max_tokens: u32,
    /// 是否把画面一起送过去（关掉可省 token / 兼容无视觉模型）
    vision: bool,
    agent: ureq::Agent,
    /// 最近一次原始响应（排障用）
    pub last_raw: Option<String>,
}

impl LlmPolicy {
    pub fn new(endpoint: Endpoint, max_tokens: u32, vision: bool) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(120))
            .build();
        LlmPolicy {
            endpoint,
            max_tokens,
            vision,
            agent,
            last_raw: None,
        }
    }

    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    pub fn vision(&self) -> bool {
        self.vision
    }

    /// 组装 Anthropic Messages 请求体
    pub fn build_body(&self, ctx: &PolicyCtx<'_>) -> Result<serde_json::Value> {
        let mut content: Vec<serde_json::Value> = Vec::new();

        // 图片块放前面 —— Anthropic 推荐 image 先于 text
        if self.vision {
            if let Some(frame) = &ctx.observation.frame {
                let bytes = std::fs::read(frame)?;
                if !bytes.is_empty() {
                    content.push(serde_json::json!({
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": "image/png",
                            "data": base64_encode(&bytes),
                        }
                    }));
                }
            }
        }

        let mut text = String::new();
        text.push_str(&format!("# 目标\n{}\n\n", ctx.goal));

        if !ctx.history.is_empty() {
            text.push_str("# 已经做过的动作（最近 5 步）\n");
            let recent: Vec<&StepRecord> = ctx.history.iter().rev().take(5).collect();
            for r in recent.into_iter().rev() {
                let note = r
                    .action
                    .note
                    .as_deref()
                    .map(|n| format!("  // {n}"))
                    .unwrap_or_default();
                text.push_str(&format!("- t{}: {}{}\n", r.tick, r.action.summary(), note));
                for w in &r.warnings {
                    text.push_str(&format!("    ⚠ {w}\n"));
                }
            }
            text.push('\n');
        }

        if let Some(n) = &ctx.observation.note {
            text.push_str(&format!("# 环境提示\n{n}\n\n"));
        }

        text.push_str(&format!(
            "# 当前是第 {} 次观察\n现在给出你的下一步动作 JSON。",
            ctx.observation.tick
        ));

        content.push(serde_json::json!({ "type": "text", "text": text }));

        Ok(serde_json::json!({
            "model": self.endpoint.model,
            "max_tokens": self.max_tokens,
            "system": system_prompt(ctx.embodiment, ctx.goal),
            "messages": [{ "role": "user", "content": content }],
        }))
    }
}

impl Policy for LlmPolicy {
    fn name(&self) -> &str {
        "llm"
    }

    fn decide(&mut self, ctx: &PolicyCtx<'_>) -> Result<Action> {
        let body = self.build_body(ctx)?;
        let url = self.endpoint.messages_url();

        let mut req = self
            .agent
            .post(&url)
            .set("content-type", "application/json")
            .set("anthropic-version", "2023-06-01")
            .set("x-api-key", &self.endpoint.api_key);

        // `PROXY_MANAGED` 是占位符：本地代理自己注入真实凭据。
        // 此时再送一个 `authorization: Bearer PROXY_MANAGED` 反而可能被网关当成
        // 真实令牌而拒绝，所以只在有真 key 时才双写两种鉴权头。
        if self.endpoint.api_key != "PROXY_MANAGED" {
            req = req.set(
                "authorization",
                &format!("Bearer {}", self.endpoint.api_key),
            );
        }

        let text = match req.send_json(&body) {
            Ok(resp) => resp
                .into_string()
                .map_err(|e| GameError::Policy(format!("读取 LLM 响应失败: {e}")))?,
            Err(ureq::Error::Status(code, resp)) => {
                let b = resp.into_string().unwrap_or_default();
                return Err(GameError::Policy(format!(
                    "LLM 返回 HTTP {code}（端点 {url}）: {}",
                    truncate(&b, 600)
                )));
            }
            Err(e) => {
                return Err(GameError::Policy(format!(
                    "调用 LLM 失败（端点 {url}）: {e}。若是本地 cc-switch 代理，先启动 cc-switch"
                )));
            }
        };

        self.last_raw = Some(text.clone());

        let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
            GameError::Policy(format!(
                "LLM 响应不是 JSON: {e}；原文: {}",
                truncate(&text, 400)
            ))
        })?;

        if let Some(err) = v.get("error") {
            return Err(GameError::Policy(format!(
                "LLM 返回错误: {}",
                truncate(&err.to_string(), 400)
            )));
        }

        let content = extract_text(&v)?;
        parse_action(&content)
    }
}

/// 给模型的系统提示词 —— 中间层的「接口说明」。
///
/// 刻意把躯体能力写进去（支持的动词、有没有视线轴），
/// 这样同一份提示词模板对 3D 游戏、2D 游戏、机器人都成立。
pub fn system_prompt(em: &Embodiment, goal: &str) -> String {
    let look = match em.look {
        LookMode::None => "无独立视线轴（yaw/pitch 会被忽略）".to_string(),
        LookMode::RelativeMouse => format!("相对鼠标位移，每度偏航约 {:.0} 像素", em.px_per_deg),
        LookMode::Servo => "云台伺服（角度指令）".to_string(),
    };
    let loco = match em.locomotion {
        Locomotion::Analog => "连续量（forward/strafe 取 -1..1 任意值）",
        Locomotion::Digital8 => "八方向数字键（forward/strafe 会被量化成 -1/0/1）",
        Locomotion::Velocity => "速度指令（线速度 / 角速度）",
    };

    format!(
        r#"你是具身智能体的决策内核。你的唯一输出是一个 JSON 动作对象 —— 不要任何解释、不要 markdown 围栏。

# 目标
{goal}

# 你的躯体
档位：{title}（id={id}）
- 视线轴：{look}
- 移动方式：{loco}
- 可用动词：{verbs}

# 动作 JSON 字段
- forward   : -1.0..1.0，+1 向前、-1 向后
- strafe    : -1.0..1.0，+1 向右、-1 向左
- yaw       : 度数，+ 向右转
- pitch     : 度数，+ 抬头
- duration_ms: 0..60000，这个动作持续多久
- hold      : 字符串数组，期间**按住**的动词（如 ["sprint"]）
- tap       : 字符串数组，期间**点按一次**的动词（如 ["jump"]）
- say       : 字符串（可选），游戏内说话
- note      : 字符串（可选），一句话说明你为什么这么做

# 规则
1. 只输出 JSON 对象，第一个字符必须是 {{。
2. 一次只做一小步：duration_ms 建议 100..1000，宁可多走几步。
3. 拿不准就先原地观察：{{"duration_ms": 0, "note": "先看看"}}。
4. 只用上面列出的动词；用不了的动词会被忽略。
5. note 用中文写，一句话，别超过 20 字。

# 输出示例
{{"forward": 1.0, "duration_ms": 400, "note": "前方开阔，直走"}}"#,
        goal = goal,
        title = em.title,
        id = em.id,
        look = look,
        loco = loco,
        verbs = em.verb_list(),
    )
}

/// 从 Anthropic 响应里取出全部 text 块
pub fn extract_text(v: &serde_json::Value) -> Result<String> {
    let arr = v.get("content").and_then(|c| c.as_array()).ok_or_else(|| {
        GameError::Policy(format!(
            "响应里没有 content 数组: {}",
            truncate(&v.to_string(), 300)
        ))
    })?;

    let mut out = String::new();
    for block in arr {
        if block.get("type").and_then(|t| t.as_str()) == Some("text") {
            if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                out.push_str(t);
            }
        }
    }
    if out.trim().is_empty() {
        return Err(GameError::Policy(format!(
            "响应里没有文本内容: {}",
            truncate(&v.to_string(), 300)
        )));
    }
    Ok(out)
}

/// 从模型输出里解析动作。
///
/// 容错三层：去围栏 → 直接解析 → 抠第一个平衡的 JSON 对象。
/// 模型经常会写「好的，我决定：{...} 这样应该能前进」，不能直接 `from_str` 整段。
pub fn parse_action(text: &str) -> Result<Action> {
    let cleaned = strip_code_fence(text);

    if let Ok(a) = serde_json::from_str::<Action>(cleaned) {
        return Ok(a.clamped());
    }

    let obj = extract_json_object(cleaned).ok_or_else(|| {
        GameError::Policy(format!(
            "模型输出里找不到 JSON 动作对象: {}",
            truncate(text, 300)
        ))
    })?;

    let a: Action = serde_json::from_str(obj).map_err(|e| {
        GameError::Policy(format!(
            "动作 JSON 解析失败: {e}；片段: {}",
            truncate(obj, 300)
        ))
    })?;
    Ok(a.clamped())
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let head: String = s.chars().take(n).collect();
    format!("{head}…（共 {} 字符）", s.chars().count())
}

/// 回放脚本的序列化载体（`--replay-script` 文件格式）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayFile {
    #[serde(default)]
    pub steps: Vec<Action>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx<'a>(goal: &'a str, em: &'a Embodiment, obs: &'a Observation) -> PolicyCtx<'a> {
        PolicyCtx {
            goal,
            embodiment: em,
            observation: obs,
            history: &[],
        }
    }

    #[test]
    fn replay_cycles_through_script() {
        let mut p = ReplayPolicy::new(vec![Action::wait(10), Action::wait(20)]);
        let em = Embodiment::two_d();
        let obs = Observation::new(1, "t");
        let c = ctx("g", &em, &obs);
        assert_eq!(p.decide(&c).unwrap().duration_ms, 10);
        assert_eq!(p.decide(&c).unwrap().duration_ms, 20);
        assert_eq!(p.decide(&c).unwrap().duration_ms, 10);
    }

    #[test]
    fn replay_default_is_nonempty_and_three_steps() {
        let p = ReplayPolicy::looping_default();
        assert_eq!(p.len(), 3);
        assert!(!p.is_empty());
    }

    #[test]
    fn replay_from_json_parses_action_array() {
        let p = ReplayPolicy::from_json(r#"[{"forward":1.0,"duration_ms":100}]"#).unwrap();
        assert_eq!(p.len(), 1);
        assert!(ReplayPolicy::from_json("not json").is_err());
    }

    #[test]
    fn parse_action_handles_plain_json() {
        let a = parse_action(r#"{"forward":1.0,"duration_ms":300}"#).unwrap();
        assert_eq!(a.forward, 1.0);
        assert_eq!(a.duration_ms, 300);
    }

    #[test]
    fn parse_action_handles_fenced_json() {
        let a = parse_action("```json\n{\"strafe\": -0.5, \"duration_ms\": 200}\n```").unwrap();
        assert_eq!(a.strafe, -0.5);
    }

    #[test]
    fn parse_action_handles_surrounding_prose() {
        let a = parse_action("好的，我决定：{\"tap\":[\"jump\"],\"duration_ms\":150} 这样能过去")
            .unwrap();
        assert_eq!(a.tap, vec![crate::action::Verb::Jump]);
    }

    #[test]
    fn parse_action_rejects_garbage() {
        let err = parse_action("我觉得应该往前走").unwrap_err();
        assert!(err.to_string().contains("找不到 JSON 动作对象"), "{err}");
    }

    #[test]
    fn parse_action_clamps_out_of_range() {
        let a = parse_action(r#"{"forward":99.0,"duration_ms":999999}"#).unwrap();
        assert_eq!(a.forward, 1.0);
        assert_eq!(a.duration_ms, 60_000);
    }

    #[test]
    fn extract_text_concatenates_text_blocks() {
        let v = serde_json::json!({
            "content": [
                {"type": "text", "text": "{\"forward\":"},
                {"type": "text", "text": "1.0}"}
            ]
        });
        assert_eq!(extract_text(&v).unwrap(), "{\"forward\":1.0}");
    }

    #[test]
    fn extract_text_errors_on_empty() {
        let v = serde_json::json!({"content": []});
        assert!(extract_text(&v).is_err());
        assert!(extract_text(&serde_json::json!({"id": "x"})).is_err());
    }

    #[test]
    fn system_prompt_mentions_embodiment_capabilities() {
        let p = system_prompt(&Embodiment::two_d(), "走到旗子");
        assert!(p.contains("走到旗子"), "{p}");
        assert!(p.contains("无独立视线轴"), "{p}");
        assert!(p.contains("八方向"), "{p}");
        assert!(p.contains("jump"), "{p}");
    }

    #[test]
    fn system_prompt_for_3d_mentions_mouse_look() {
        let p = system_prompt(&Embodiment::three_d(), "找钻石");
        assert!(p.contains("相对鼠标位移"), "{p}");
        assert!(p.contains("连续量"), "{p}");
    }

    #[test]
    fn build_body_includes_system_and_user_turns() {
        let ep = Endpoint {
            base_url: "http://127.0.0.1:15721".into(),
            api_key: "PROXY_MANAGED".into(),
            model: "m".into(),
            source: "env".into(),
            reachable: true,
            note: None,
        };
        let p = LlmPolicy::new(ep, 256, false);
        let em = Embodiment::three_d();
        let obs = Observation::new(1, "Minecraft");
        let c = ctx("砍树", &em, &obs);
        let body = p.build_body(&c).unwrap();

        assert_eq!(body["model"], "m");
        assert_eq!(body["max_tokens"], 256);
        assert!(body["system"].as_str().unwrap().contains("砍树"));
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        let content = msgs[0]["content"].as_array().unwrap();
        // vision 关掉时只有一个 text 块
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "text");
        assert!(content[0]["text"].as_str().unwrap().contains("砍树"));
    }

    #[test]
    fn build_body_puts_image_before_text_when_vision_on() {
        let ep = Endpoint {
            base_url: "http://127.0.0.1:15721".into(),
            api_key: "PROXY_MANAGED".into(),
            model: "m".into(),
            source: "env".into(),
            reachable: true,
            note: None,
        };
        let dir = std::env::temp_dir().join("lgame-test-vision");
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("f.png");
        std::fs::write(&png, b"\x89PNG\r\n\x1a\nfake").unwrap();

        let p = LlmPolicy::new(ep, 256, true);
        let em = Embodiment::three_d();
        let obs = Observation::new(1, "x").with_frame(&png);
        let c = ctx("g", &em, &obs);
        let body = p.build_body(&c).unwrap();
        let content = body["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "image");
        assert_eq!(content[1]["type"], "text");
        assert_eq!(content[0]["source"]["type"], "base64");
        assert_eq!(content[0]["source"]["media_type"], "image/png");
        assert!(!content[0]["source"]["data"].as_str().unwrap().is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn build_body_reports_history_and_warnings() {
        let ep = Endpoint {
            base_url: "http://127.0.0.1:15721".into(),
            api_key: "PROXY_MANAGED".into(),
            model: "m".into(),
            source: "env".into(),
            reachable: true,
            note: None,
        };
        let p = LlmPolicy::new(ep, 256, false);
        let em = Embodiment::two_d();
        let obs = Observation::new(2, "t");
        let hist = vec![StepRecord {
            tick: 1,
            action: Action {
                forward: 1.0,
                duration_ms: 300,
                note: Some("直走".into()),
                ..Default::default()
            },
            frame: None,
            warnings: vec!["躯体 `2d` 不支持动词 `crouch`，已忽略".into()],
        }];
        let c = PolicyCtx {
            goal: "g",
            embodiment: &em,
            observation: &obs,
            history: &hist,
        };
        let body = p.build_body(&c).unwrap();
        let text = body["messages"][0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(text.contains("已经做过的动作"), "{text}");
        assert!(text.contains("直走"), "{text}");
        assert!(text.contains("不支持动词"), "{text}");
    }

    #[test]
    fn boxed_policy_forwards_to_inner() {
        let em = Embodiment::two_d();
        let obs = Observation::new(1, "t");
        let c = ctx("g", &em, &obs);
        let mut boxed: Box<dyn Policy> = Box::new(ReplayPolicy::new(vec![Action::wait(42)]));
        assert_eq!(boxed.name(), "replay");
        assert_eq!(boxed.decide(&c).unwrap().duration_ms, 42);
    }
}
