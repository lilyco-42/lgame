//! # lgame-tools — 游戏控制工具集（lilyco 四端）
//!
//! 把 [`lgame_core`] 的中间层包成 lilyco 的 `App`，于是**一个二进制**同时是：
//!
//! ```text
//! l3d observe --target minecraft          # CLI 子命令
//! l3d --gui                               # Web 控制台（SSE 进度）
//! l3d --tui                               # TUI 命令选择页
//! l3d --mcp                               # MCP stdio 服务器（Agent 直接调用）
//! ```
//!
//! ## 工具一览
//!
//! | 工具 | 分级 | 作用 |
//! |---|---|---|
//! | `doctor` | T0 | 环境自检：可见窗口 / LLM 端点 / 内置目标 |
//! | `targets` | T0 | 内置游戏目标与躯体档位 |
//! | `observe` | T0 | 抓一帧画面 |
//! | `act` | T1 | 执行一个动作（默认试跑） |
//! | `step` | T1 | 走一步：观察 → 决策 →（可选）执行 |
//! | `run` | T1 | 连续走多步，落 trace.jsonl |
//! | `think` | T1 | 用宿主 MCP 客户端的 LLM 决策（采样桥，无图） |
//!
//! ## 安全边界
//!
//! 所有会注入键鼠的工具都有 `apply: bool`，**缺省即试跑**。
//! 这是刻意的：`#[derive(App)]` 的 `default` 只在 CLI 面生效，
//! MCP 的 `tools/call` 缺参会兜底成 `false` —— 所以用「正向 opt-in」
//! （`apply=true` 才动手）而不是「默认 dry-run」，两种调用面的行为才一致。

pub mod convert;
pub mod registry;
pub mod tools;

pub use registry::{build_registry_with_policy, policy_for, run_binary};
