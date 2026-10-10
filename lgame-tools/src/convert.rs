//! 中间层错误 → lilyco `AppError` 的映射。
//!
//! # 为什么不是 `impl From<GameError> for AppError`
//!
//! 因为**孤儿规则不允许**：`AppError` 是 lilyco 的、`GameError` 是 lgame-core 的，
//! 两个都是外部类型，在第三个 crate 里没法给 `From` 写 impl（E0117）。
//!
//! 所以用一个**本 crate 定义**的 trait [`IntoLgame`] 来承载转换：
//! 工具函数里写 `something().lg()?`，语义和 `?` 一样，但绕开了孤儿规则。
//!
//! # 映射规则（决定 Agent 看到什么终态）
//!
//! | 中间层错误 | lilyco 终态 | 为什么 |
//! |---|---|---|
//! | `Config` / `Target` | `InvalidArg` | 用户改一下参数就能好 |
//! | `Capture` / `Input` / `Driver` | `Runtime` | 环境问题，值得重试或换环境 |
//! | `Policy` | `Runtime` | LLM 抽风 |
//! | `Io` / `Json` | `Io` / `InvalidInput` | 各自的原生分类 |
//! | `Cancelled` | `Cancelled` | 原样透传 |

use lgame_core::GameError;
use lilyco::prelude::AppError;

/// 把中间层错误翻译成 lilyco 的终态错误
pub fn to_app(e: GameError) -> AppError {
    match e {
        GameError::Config(m) | GameError::Target(m) => AppError::InvalidArg(m),
        GameError::Capture(m) | GameError::Input(m) | GameError::Driver(m) => AppError::Runtime(m),
        GameError::Policy(m) => AppError::Runtime(format!("决策失败: {m}")),
        GameError::Io(m) => AppError::Io(std::io::Error::other(m)),
        GameError::Json(m) => AppError::InvalidInput(format!("JSON 错误: {m}")),
        GameError::Cancelled => AppError::Cancelled,
    }
}

/// 让 `lgame_core::Result<T>` 能直接 `.lg()?` 到 `AppError`
///
/// trait 定义在本 crate，所以给外部的 `Result` 写 impl 是合法的。
pub trait IntoLgame<T> {
    /// 转换错误类型（等价于 `map_err(to_app)`）
    fn lg(self) -> Result<T, AppError>;
}

impl<T> IntoLgame<T> for Result<T, GameError> {
    fn lg(self) -> Result<T, AppError> {
        self.map_err(to_app)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_and_target_become_invalid_arg() {
        assert!(matches!(
            to_app(GameError::Config("x".into())),
            AppError::InvalidArg(_)
        ));
        assert!(matches!(
            to_app(GameError::Target("x".into())),
            AppError::InvalidArg(_)
        ));
    }

    #[test]
    fn environment_failures_become_runtime() {
        assert!(matches!(
            to_app(GameError::Capture("x".into())),
            AppError::Runtime(_)
        ));
        assert!(matches!(
            to_app(GameError::Input("x".into())),
            AppError::Runtime(_)
        ));
        assert!(matches!(
            to_app(GameError::Driver("x".into())),
            AppError::Runtime(_)
        ));
        match to_app(GameError::Policy("模型抽风".into())) {
            AppError::Runtime(m) => assert!(m.contains("决策失败"), "{m}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn io_and_json_map_to_their_own_variants() {
        assert!(matches!(to_app(GameError::Io("x".into())), AppError::Io(_)));
        assert!(matches!(
            to_app(GameError::Json("x".into())),
            AppError::InvalidInput(_)
        ));
    }

    #[test]
    fn cancellation_is_preserved() {
        assert!(matches!(to_app(GameError::Cancelled), AppError::Cancelled));
    }

    #[test]
    fn lg_adapts_results_and_keeps_ok_values() {
        let ok: Result<u8, GameError> = Ok(7);
        assert_eq!(ok.lg().unwrap(), 7);

        let err: Result<u8, GameError> = Err(GameError::Config("坏参数".into()));
        match err.lg() {
            Err(AppError::InvalidArg(m)) => assert_eq!(m, "坏参数"),
            other => panic!("{other:?}"),
        }
    }
}
