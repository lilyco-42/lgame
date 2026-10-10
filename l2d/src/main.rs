//! # l2d — 2D 游戏角色控制
//!
//! 把 AI 的决策落到**2D 平台跳跃 / 俯视**游戏里的角色动作上：
//! `forward`/`strafe` 被量化成八方向数字键（WASD），`jump`/`attack` → Z / X。
//! 2D 躯体**没有独立视线轴**，所以 `yaw`/`pitch` 会被忽略并给出警告
//! （警告会进 trace，不会静默吞掉）。
//!
//! 与 [`l3d`](../l3d) 的唯一差别是**默认躯体档位**：`l2d` 用 `2d`（无视线轴、
//! 八方向数字键），`l3d` 用 `3d`（有视线轴、连续量）。工具集完全共享。
//!
//! ## 用法
//!
//! ```bash
//! # 0) 先自检
//! l2d doctor
//!
//! # 1) 看内置目标（aic / stronghold / tsukishita / ...）
//! l2d targets
//!
//! # 2) 抓一帧确认窗口对不对
//! l2d observe --target aic
//!
//! # 3) 试跑一步（只决策，不按键）
//! l2d step --target aic --goal "往右走并跳过缺口"
//!
//! # 4) 真的驱动
//! l2d run --target aic --goal "清掉这一波敌人" --steps 30 --apply
//!
//! # 5) 不启动 cc-switch 也能验证链路
//! l2d run --target aic --goal "demo" --steps 6 --replay --apply
//!
//! # 6) 给 Agent 用：MCP stdio 服务器
//! l2d --mcp
//! ```

fn main() {
    // 唯一入口：设定默认躯体 → 探测后端 → 按后端注入安全策略 → 交给 lilyco 分发。
    // 这里**不能**自己调 `run_registry_with` —— 它会按后端覆盖策略（MCP → 拒 T1）。
    lgame_tools::run_binary("l2d", "2d");
}
