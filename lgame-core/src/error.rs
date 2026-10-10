//! 错误类型：中间层的失败必须能说清「哪一环断了」。
//!
//! 刻意分成几类而不是一个大 `String` —— 上层（lilyco 的 `AppError`）要按类
//! 映射成不同的终态，Agent 也要能判断「是重试还是改参数」。

use std::fmt;

/// 中间层错误
#[derive(Debug, Clone)]
pub enum GameError {
    /// 参数 / 配置错误：用户改一下就能好
    Config(String),
    /// 目标不可用：窗口找不到、游戏没开、进程不在
    Target(String),
    /// 截屏失败
    Capture(String),
    /// 输入注入失败：权限不足（macOS 辅助功能 / Linux uinput）、焦点不在目标上
    Input(String),
    /// 决策失败：LLM 不可达、返回不是合法动作
    Policy(String),
    /// 驱动执行失败
    Driver(String),
    /// 文件 / IO
    Io(String),
    /// JSON 解析
    Json(String),
    /// 被上层取消
    Cancelled,
}

impl fmt::Display for GameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GameError::Config(m) => write!(f, "配置错误: {m}"),
            GameError::Target(m) => write!(f, "目标不可用: {m}"),
            GameError::Capture(m) => write!(f, "截屏失败: {m}"),
            GameError::Input(m) => write!(f, "输入注入失败: {m}"),
            GameError::Policy(m) => write!(f, "决策失败: {m}"),
            GameError::Driver(m) => write!(f, "驱动失败: {m}"),
            GameError::Io(m) => write!(f, "IO 错误: {m}"),
            GameError::Json(m) => write!(f, "JSON 错误: {m}"),
            GameError::Cancelled => write!(f, "已取消"),
        }
    }
}

impl std::error::Error for GameError {}

impl From<std::io::Error> for GameError {
    fn from(e: std::io::Error) -> Self {
        GameError::Io(e.to_string())
    }
}

impl From<serde_json::Error> for GameError {
    fn from(e: serde_json::Error) -> Self {
        GameError::Json(e.to_string())
    }
}

/// 中间层统一返回类型
pub type Result<T> = std::result::Result<T, GameError>;
