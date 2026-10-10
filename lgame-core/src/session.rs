//! 会话循环 —— 把四块地基拼起来：观察 → 决策 → 执行 → 记录。
//!
//! ```text
//!   ┌─────────────┐   Observation   ┌──────────┐
//!   │  Driver     │ ───────────────▶│  Policy  │
//!   │ (看 / 做)    │                 │ (想)      │
//!   └─────────────┘ ◀───────────────└──────────┘
//!          │            Action
//!          ▼
//!      StepRecord ──▶ Trace (JSONL) ──▶ 下一轮提示词
//! ```
//!
//! 这一层刻意不认识 lilyco、不认识窗口、不认识 LLM —— 只认
//! [`Driver`] 和 [`Policy`] 两个 trait。所以换成机器人只是换实现。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;

use crate::driver::Driver;
use crate::error::Result;
use crate::policy::{Policy, PolicyCtx};
use crate::trace::{StepRecord, Trace};

/// 会话参数
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// 高层目标（自然语言）
    pub goal: String,
    /// 最多走多少步
    pub max_steps: u32,
    /// 步与步之间的间隔（给游戏留出渲染时间）
    pub step_delay_ms: u64,
    /// 画面与轨迹的落盘目录
    pub out_dir: PathBuf,
    /// 只决策不执行（安全试跑）
    pub dry_run: bool,
}

impl Default for SessionConfig {
    fn default() -> Self {
        SessionConfig {
            goal: "探索环境".to_string(),
            max_steps: 10,
            step_delay_ms: 300,
            out_dir: PathBuf::from("lgame-captures"),
            dry_run: true,
        }
    }
}

/// 会话结果
#[derive(Debug, Clone)]
pub struct SessionOutcome {
    pub steps: Vec<StepRecord>,
    /// 轨迹文件路径
    pub trace_path: PathBuf,
    /// 为什么停下：`达到步数上限` / `被外部停止`
    pub stopped_because: String,
}

impl SessionOutcome {
    pub fn step_count(&self) -> usize {
        self.steps.len()
    }

    /// 给 Agent / CLI 的结果摘要
    pub fn to_json(&self) -> serde_json::Value {
        let tail: Vec<&StepRecord> = self.steps.iter().rev().take(10).collect();
        serde_json::json!({
            "steps": self.step_count(),
            "stopped_because": self.stopped_because,
            "trace": self.trace_path.display().to_string(),
            "last_actions": tail.into_iter().rev().map(|r| {
                serde_json::json!({
                    "tick": r.tick,
                    "action": r.action,
                    "summary": r.action.summary(),
                    "warnings": r.warnings,
                })
            }).collect::<Vec<_>>(),
        })
    }
}

/// 外部停止开关（Ctrl-C / lilyco 取消 → 置位）
#[derive(Debug, Clone, Default)]
pub struct Control {
    stop: Arc<AtomicBool>,
}

impl Control {
    pub fn new() -> Self {
        Control {
            stop: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 请求停止（下一步之前生效）
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    pub fn is_stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    /// 供 `ctrlc` 之类的信号处理闭包捕获
    pub fn handle(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.stop)
    }
}

/// 跑一次会话
///
/// `on_step(第几步, 这一步的记录)` 用于实时上报进度（CLI 打印 / MCP 通知）。
/// 步号用 `u32` 而不是 `usize` —— 它与 [`SessionConfig::max_steps`] 同一个类型，
/// 调用方不需要为了「这是第几步」做一次转换。
pub fn run<D, P>(
    driver: &mut D,
    policy: &mut P,
    cfg: &SessionConfig,
    control: &Control,
    on_step: &mut dyn FnMut(u32, &StepRecord),
) -> Result<SessionOutcome>
where
    D: Driver,
    P: Policy,
{
    std::fs::create_dir_all(&cfg.out_dir)?;
    let mut trace = Trace::create(&cfg.out_dir)?;
    let trace_path = trace.path().to_path_buf();

    let mut history: Vec<StepRecord> = Vec::new();
    let mut stopped_because = "达到步数上限".to_string();

    for step in 1..=cfg.max_steps {
        if control.is_stopped() {
            stopped_because = "被外部停止".to_string();
            break;
        }

        // 1) 看
        let obs = driver.observe(&cfg.out_dir)?;

        // 2) 想（把躯体克隆出来，避免与后面的可变借用打架）
        let embodiment = driver.embodiment().clone();
        let action = {
            let ctx = PolicyCtx {
                goal: &cfg.goal,
                embodiment: &embodiment,
                observation: &obs,
                history: &history,
            };
            policy.decide(&ctx)?
        };

        // 3) 做
        let warnings = if cfg.dry_run {
            vec!["dry-run：只决策未执行，没有注入任何输入".to_string()]
        } else {
            driver.execute(&action)?
        };

        // 4) 记
        let rec = StepRecord::new(obs.tick, action)
            .with_frame(obs.frame.clone())
            .with_warnings(warnings);
        trace.append(&rec)?;
        on_step(step, &rec);
        history.push(rec);

        if cfg.step_delay_ms > 0 && step < cfg.max_steps {
            sleep(Duration::from_millis(cfg.step_delay_ms));
        }
    }

    Ok(SessionOutcome {
        steps: history,
        trace_path,
        stopped_because,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::Action;
    use crate::embodiment::Embodiment;
    use crate::observation::Observation;
    use crate::policy::ReplayPolicy;

    /// 假驱动：不碰屏幕，直接产出可预测的观察
    struct FakeDriver {
        em: Embodiment,
        tick: u64,
        executed: Vec<Action>,
    }

    impl FakeDriver {
        fn new() -> Self {
            FakeDriver {
                em: Embodiment::two_d(),
                tick: 0,
                executed: Vec::new(),
            }
        }
    }

    impl Driver for FakeDriver {
        fn target(&self) -> &str {
            "fake"
        }
        fn embodiment(&self) -> &Embodiment {
            &self.em
        }
        fn observe(&mut self, _out_dir: &std::path::Path) -> Result<Observation> {
            self.tick += 1;
            Ok(Observation::new(self.tick, "fake"))
        }
        fn execute(&mut self, action: &Action) -> Result<Vec<String>> {
            self.executed.push(action.clone());
            Ok(Vec::new())
        }
    }

    fn tmp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn loop_runs_max_steps_and_records_everything() {
        let dir = tmp_dir("lgame-test-session-basic");
        let mut driver = FakeDriver::new();
        let mut policy = ReplayPolicy::looping_default();
        let cfg = SessionConfig {
            goal: "走".into(),
            max_steps: 3,
            step_delay_ms: 0,
            out_dir: dir.clone(),
            dry_run: false,
        };
        let control = Control::new();
        let mut seen = 0usize;

        let out = run(&mut driver, &mut policy, &cfg, &control, &mut |_, _| {
            seen += 1
        })
        .unwrap();

        assert_eq!(out.step_count(), 3);
        assert_eq!(seen, 3);
        assert_eq!(out.stopped_because, "达到步数上限");
        assert_eq!(driver.executed.len(), 3);
        assert!(out.trace_path.exists());
        let lines = std::fs::read_to_string(&out.trace_path)
            .unwrap()
            .lines()
            .count();
        assert_eq!(lines, 3);

        let j = out.to_json();
        assert_eq!(j["steps"], 3);
        assert_eq!(j["last_actions"].as_array().unwrap().len(), 3);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dry_run_decides_but_never_injects() {
        let dir = tmp_dir("lgame-test-session-dry");
        let mut driver = FakeDriver::new();
        let mut policy = ReplayPolicy::looping_default();
        let cfg = SessionConfig {
            goal: "走".into(),
            max_steps: 2,
            step_delay_ms: 0,
            out_dir: dir.clone(),
            dry_run: true,
        };
        let control = Control::new();

        let out = run(&mut driver, &mut policy, &cfg, &control, &mut |_, _| {}).unwrap();

        assert_eq!(out.step_count(), 2);
        assert!(driver.executed.is_empty(), "dry-run 不该注入任何输入");
        assert!(
            out.steps[0].warnings.iter().any(|w| w.contains("dry-run")),
            "{:?}",
            out.steps[0].warnings
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn control_can_stop_the_loop_early() {
        let dir = tmp_dir("lgame-test-session-stop");
        let mut driver = FakeDriver::new();
        let mut policy = ReplayPolicy::looping_default();
        let cfg = SessionConfig {
            goal: "走".into(),
            max_steps: 100,
            step_delay_ms: 0,
            out_dir: dir.clone(),
            dry_run: false,
        };
        let control = Control::new();
        let c2 = control.clone();

        let out = run(&mut driver, &mut policy, &cfg, &control, &mut |step, _| {
            if step == 2 {
                c2.stop();
            }
        })
        .unwrap();

        assert_eq!(out.step_count(), 2);
        assert_eq!(out.stopped_because, "被外部停止");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn policy_errors_propagate_instead_of_being_swallowed() {
        struct BadPolicy;
        impl Policy for BadPolicy {
            fn name(&self) -> &str {
                "bad"
            }
            fn decide(&mut self, _ctx: &PolicyCtx<'_>) -> Result<Action> {
                Err(crate::error::GameError::Policy("模型抽风".into()))
            }
        }

        let dir = tmp_dir("lgame-test-session-err");
        let mut driver = FakeDriver::new();
        let mut policy = BadPolicy;
        let cfg = SessionConfig {
            max_steps: 5,
            step_delay_ms: 0,
            out_dir: dir.clone(),
            ..Default::default()
        };
        let err = run(
            &mut driver,
            &mut policy,
            &cfg,
            &Control::new(),
            &mut |_, _| {},
        )
        .unwrap_err();
        assert!(err.to_string().contains("模型抽风"), "{err}");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
