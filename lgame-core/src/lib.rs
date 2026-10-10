//! # lgame-core — AI 控制实体的中间层
//!
//! 把「AI 想做什么」和「具体怎么动」拆开。游戏是第一个落地场景，
//! 机器人是同一套抽象的另一个档位。
//!
//! ## 四块地基
//!
//! | 模块 | 职责 | 是否与平台绑定 |
//! |---|---|---|
//! | [`action`] | 动作空间：`forward/strafe/yaw/pitch` + 语义动词 | 否 |
//! | [`embodiment`] | 躯体档位：有没有视线轴、连续量还是八方向 | 否 |
//! | [`driver`] | 看一帧 + 做一个动作 | **是**（屏幕档位 / 将来的机器人档位） |
//! | [`policy`] | 看 → 决定做什么 | 否（LLM / 脚本回放） |
//!
//! 加一个 [`session`] 把四者串成循环，[`trace`] 把每一步记成 JSONL。
//!
//! ## 为什么动作不是「按 W 键」
//!
//! ```text
//! 3D 第一人称：forward=1  → 按住 W       + 鼠标相对位移
//! 2D 平台跳跃：forward=1  → 按住 W       （没有视线轴）
//! 机器人底盘  ：forward=1  → 线速度 0.5 m/s + 云台角速度
//! ```
//!
//! 同一份 `Action`，三种落地方式。策略层（提示词、LLM）完全不用改。
//!
//! ## 最小用法
//!
//! ```no_run
//! use lgame_core::{Control, Embodiment, ReplayPolicy, SessionConfig};
//!
//! # fn main() -> lgame_core::Result<()> {
//! let mut driver = lgame_core::ScreenDriver::new(
//!     "Minecraft",                    // 窗口标题（子串匹配）
//!     Embodiment::three_d(),          // 躯体档位
//!     Some(1280),                     // 送 LLM 前缩到 1280 宽
//! )?;
//!
//! // 先用离线脚本把链路跑通，再换成 LlmPolicy 接本地 cc-switch
//! let mut policy = ReplayPolicy::looping_default();
//!
//! let cfg = SessionConfig {
//!     goal: "走出这片树林".into(),
//!     max_steps: 10,
//!     dry_run: true,                  // 先不真的按键
//!     ..Default::default()
//! };
//!
//! let outcome = lgame_core::session::run(
//!     &mut driver, &mut policy, &cfg, &Control::new(), &mut |step, rec| {
//!         println!("第 {step} 步：{}", rec.summary());
//!     },
//! )?;
//! println!("{} 步，轨迹在 {}", outcome.step_count(), outcome.trace_path.display());
//! # Ok(())
//! # }
//! ```
//!
//! 注：这段依赖 `screen` 特性（默认开启）。`--no-default-features` 下
//! `ScreenDriver` 不存在，本示例不适用 —— 此时应直接实现 [`Driver`]。
//!
//! ## 接本地免费 API（cc-switch）
//!
//! ```no_run
//! # fn main() -> lgame_core::Result<()> {
//! let endpoint = lgame_core::ccswitch::discover()?;   // 读 ~/.claude/settings.json
//! let policy = lgame_core::LlmPolicy::new(endpoint, 512, true);  // true = 带画面
//! # let _ = policy;
//! # Ok(())
//! # }
//! ```
//!
//! 端点优先级：环境变量 `LGAME_LLM_BASE_URL` / `LGAME_LLM_API_KEY` / `LGAME_LLM_MODEL`
//! 高于 `~/.claude/settings.json`。详见 [`ccswitch`]。

pub mod action;
pub mod ccswitch;
pub mod driver;
pub mod embodiment;
pub mod error;
pub mod observation;
pub mod policy;
pub mod session;
pub mod targets;
pub mod trace;
pub mod util;

#[cfg(feature = "screen")]
pub mod capture;
#[cfg(feature = "screen")]
pub mod input;

pub use action::{Action, Input, Keymap, MouseBtn, Verb};
pub use ccswitch::Endpoint;
pub use driver::Driver;
pub use embodiment::{Embodiment, Locomotion, LookMode};
pub use error::{GameError, Result};
pub use observation::Observation;
pub use policy::{LlmPolicy, Policy, PolicyCtx, ReplayPolicy};
pub use session::{Control, SessionConfig, SessionOutcome};
pub use targets::Target;
pub use trace::{StepRecord, Trace};

#[cfg(feature = "screen")]
pub use driver::ScreenDriver;

/// 本 crate 版本
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 编译时是否启用了屏幕档位（截屏 + 输入注入）
///
/// 供上层（`lgame-tools` 的 `doctor`）如实报告能力，而不是硬编码 `cfg!`
/// —— 上游 crate 的 feature 与下游不是同一个。
pub const SCREEN_ENABLED: bool = cfg!(feature = "screen");

/// 常用导入
pub mod prelude {
    pub use crate::action::{Action, Input, Keymap, MouseBtn, Verb};
    pub use crate::ccswitch::{self, Endpoint};
    pub use crate::driver::Driver;
    pub use crate::embodiment::{self, Embodiment, Locomotion, LookMode};
    pub use crate::error::{GameError, Result};
    pub use crate::observation::Observation;
    pub use crate::policy::{LlmPolicy, Policy, PolicyCtx, ReplayPolicy};
    pub use crate::session::{self, Control, SessionConfig, SessionOutcome};
    pub use crate::targets::{self, Target};
    pub use crate::trace::{StepRecord, Trace};

    #[cfg(feature = "screen")]
    pub use crate::capture;
    #[cfg(feature = "screen")]
    pub use crate::driver::ScreenDriver;
    #[cfg(feature = "screen")]
    pub use crate::input::Injector;
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_not_empty() {
        assert!(!super::VERSION.is_empty());
    }

    /// 内置目标表里的躯体档位必须都能解析出来（防止改名后悄悄失效）
    #[test]
    fn every_builtin_target_resolves_its_embodiment() {
        for t in super::targets::builtin() {
            assert!(
                t.embodiment().is_some(),
                "目标 `{}` 的躯体 `{}` 解析失败",
                t.id,
                t.embodiment
            );
        }
    }

    /// 默认躯体档位必须能解析（`l3d` / `l2d` 的 main 依赖它）
    #[test]
    fn default_embodiment_always_resolves() {
        let e = super::embodiment::default_embodiment();
        assert!(super::embodiment::Embodiment::by_id(&e.id).is_some());
    }
}
