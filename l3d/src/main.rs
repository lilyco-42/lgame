//! # l3d — 3D 游戏角色控制
//!
//! 把 AI 的决策落到**第一/第三人称 3D** 游戏里的角色动作上：
//! `forward`/`strafe` → WASD，`yaw`/`pitch` → 相对鼠标位移，`jump`/`sprint` → 空格 / Shift。
//!
//! 与 [`l2d`](../l2d) 的唯一差别是**默认躯体档位**：`l3d` 用 `3d`（有独立视线轴、
//! 移动是连续量），`l2d` 用 `2d`（无视线轴、八方向数字键）。工具集完全共享。
//!
//! ## 用法
//!
//! ```bash
//! # 0) 先自检：看得到哪些窗口、本地 LLM 端点通不通
//! l3d doctor
//!
//! # 1) 看内置目标（minecraft / zombie-raid / ...）
//! l3d targets
//!
//! # 2) 抓一帧看看目标对不对（T0 只读）
//! l3d observe --target minecraft
//!
//! # 3) 试跑一步：观察 → 让本地 cc-switch 的模型决策 → 只打印不按键
//! l3d step --target minecraft --goal "走出这片树林"
//!
//! # 4) 确认没问题再真的驱动
//! l3d run --target minecraft --goal "砍三棵树" --steps 20 --apply
//!
//! # 5) 不启动 cc-switch 也能验证链路（离线脚本策略）
//! l3d run --target minecraft --goal "demo" --steps 6 --replay --apply
//!
//! # 6) 给 Agent 用：MCP stdio 服务器
//! l3d --mcp
//! ```
//!
//! ## 本地免费 API
//!
//! 默认走 cc-switch 的本地代理（地址从 `~/.claude/settings.json` 的
//! `env.ANTHROPIC_BASE_URL` 读，默认 `http://127.0.0.1:15721`）。
//! 也可用环境变量覆盖：`LGAME_LLM_BASE_URL` / `LGAME_LLM_API_KEY` / `LGAME_LLM_MODEL`。

fn main() {
    // 唯一入口：设定默认躯体 → 探测后端 → 按后端注入安全策略 → 交给 lilyco 分发。
    // 这里**不能**自己调 `run_registry_with` —— 它会按后端覆盖策略（MCP → 拒 T1）。
    lgame_tools::run_binary("l3d", "3d");
}
