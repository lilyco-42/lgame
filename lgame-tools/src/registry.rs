//! 注册表装配 —— 加一个工具只在这里加一行。
//!
//! 四端共用这一份：CLI 生成子命令、TUI 生成选择页、Web 用 `?cmd=` 切换、
//! MCP 一次 `tools/list` 全返回。

use std::sync::Arc;

use lilyco::prelude::*;

use crate::tools;

/// 构建「游戏控制」域的注册表（使用给定安全策略）
///
/// **策略必须在 `register` 之前就位** —— `Registry` 的安全门是在注册那一刻
/// 把 handler 包住的，事后换策略只能叠门（旧门先拒）。
pub fn build_registry_with_policy(policy: Arc<dyn SafetyPolicy>) -> Registry {
    let mut reg = Registry::new().with_policy(policy);

    let cmds: Vec<RegisteredCommand> = vec![
        // 只读面（T0）—— 自动化面可直接放行
        RegisteredCommand::from_app::<tools::Doctor>(),
        RegisteredCommand::from_app::<tools::Targets>(),
        RegisteredCommand::from_app::<tools::Observe>(),
        // 会动世界的面（T1）—— 默认 dry-run，`apply=true` 才真注入
        RegisteredCommand::from_app::<tools::Act>(),
        RegisteredCommand::from_app::<tools::Step>(),
        RegisteredCommand::from_app::<tools::Run>(),
        RegisteredCommand::from_app::<tools::Think>(),
    ];

    for c in cmds {
        let name = c.name.clone();
        reg.register(c)
            .unwrap_or_else(|e| panic!("注册命令 `{name}` 失败: {e}"));
    }
    reg
}

/// 安全策略选择。
///
/// # 与 `lfiles` 的差异（有意的）
///
/// `lfiles`（文件工具）的 MCP 面用 `DenyElevated` —— Agent 不该悄悄改你的文件。
/// 但 lgame 的**存在意义**就是让 Agent 驱动输入，MCP 面若也拒绝 T1，
/// 这个工具就没有使用场景了。
///
/// 所以这里两端都用 `Interactive`：放行 T1（需确认），仍拒绝 T3（永不自动化）。
/// 安全边界改由**默认值**承担 —— 所有会注入输入的工具有 `apply: bool`，
/// 缺省为 `false`（只决策、不按键）。Agent 必须显式 `apply=true` 才动得了键鼠。
///
/// 保留 `backend` 参数是为了将来分面收紧时不用改调用方。
pub fn policy_for(_backend: lilyco::Backend) -> Arc<dyn SafetyPolicy> {
    Arc::new(Interactive)
}

/// 启动一个 lgame 二进制（`l3d` / `l2d` 的唯一入口）。
///
/// 顺序很关键，三步都不能省：
///
/// 1. `set_default(躯体)` —— 必须在建注册表**之前**，工具在 `embodiment=auto`
///    时读它（`l3d` 给 `3d`、`l2d` 给 `2d`）。
/// 2. `build_registry_with_policy(Interactive)` —— **内层门**。门是在 `register`
///    那一刻包住 handler 的，事后换不了；若这里用默认的 `DenyElevated`，
///    T1 工具会先被内层拒掉，后面套几层 `Interactive` 都没用。
/// 3. `run_registry_with_policy(…)` —— **外层门**，按后端注入。
///
/// # 为什么不是 `run_registry_with`
///
/// 那个函数会按后端**覆盖**策略：MCP → `DenyElevated` → `act`/`step`/`run`/`think`
/// 全部被拒。而「让 Agent 驱动输入」正是本工具存在的意义，MCP 面被拒就没场景了。
/// 所以统一走 `run_registry_with_policy`，两端都用 `Interactive`；
/// 真正的安全边界由 `apply: bool` 默认 `false`（只决策不按键）承担。
pub fn run_binary(app_name: &str, default_embodiment: &str) {
    lgame_core::embodiment::set_default(default_embodiment);

    let backend = lilyco::detect_registry_backend();
    let policy = policy_for(backend);
    let registry = build_registry_with_policy(Arc::clone(&policy));

    lilyco::run_registry_with_policy(app_name, registry, backend, policy);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(reg: &Registry) -> Vec<String> {
        let mut v: Vec<String> = reg.iter().map(|c| c.name.clone()).collect();
        v.sort();
        v
    }

    #[test]
    fn registry_has_all_tools() {
        let reg = build_registry_with_policy(Arc::new(Interactive));
        assert_eq!(
            names(&reg),
            vec![
                "act".to_string(),
                "doctor".to_string(),
                "observe".to_string(),
                "run".to_string(),
                "step".to_string(),
                "targets".to_string(),
                "think".to_string(),
            ]
        );
        assert_eq!(reg.visible().count(), 7, "所有工具都应出现在 tools/list 里");
    }

    /// 只读工具必须是 T0（自动化面直接放行），会动世界的必须是 T1（需确认）
    #[test]
    fn safety_tiers_are_as_declared() {
        use lilyco::prelude::SafetyTier;
        let reg = build_registry_with_policy(Arc::new(Interactive));

        for read_only in ["doctor", "targets", "observe"] {
            let c = reg.get(read_only).expect(read_only);
            assert_eq!(
                c.schema.safety,
                SafetyTier::ReadOnly,
                "{read_only} 应该是 T0"
            );
        }
        for confirming in ["act", "step", "run", "think"] {
            let c = reg.get(confirming).expect(confirming);
            assert_eq!(
                c.schema.safety,
                SafetyTier::Confirm,
                "{confirming} 应该是 T1"
            );
        }
    }

    /// 每个工具都必须有 handler，否则 MCP `tools/call` 会失败
    #[test]
    fn every_tool_has_a_handler() {
        let reg = build_registry_with_policy(Arc::new(Interactive));
        for c in reg.iter() {
            assert!(c.handler.is_some(), "`{}` 没有 handler", c.name);
        }
    }

    /// 每个工具都必须有非空描述 —— 那是 Agent 唯一的说明书
    #[test]
    fn every_tool_has_a_description() {
        let reg = build_registry_with_policy(Arc::new(Interactive));
        for c in reg.iter() {
            assert!(
                c.schema.about.len() > 40,
                "`{}` 的 about 太短（{} 字符），Agent 看不懂",
                c.name,
                c.schema.about.len()
            );
        }
    }

    /// `apply` 必须在 schema 里默认 false —— 这是安全边界，不能被改掉
    #[test]
    fn apply_defaults_to_dry_run() {
        let reg = build_registry_with_policy(Arc::new(Interactive));
        for name in ["act", "step", "run", "think"] {
            let c = reg.get(name).expect(name);
            let apply = c
                .schema
                .args
                .iter()
                .find(|a| a.name == "apply")
                .unwrap_or_else(|| panic!("`{name}` 缺少 apply 开关"));
            // 没有声明 default → from_args 兜底 false → 试跑。这正是我们要的。
            assert!(
                apply.default.is_none() || apply.default == Some(serde_json::json!(false)),
                "`{name}` 的 apply 默认值必须等价于 false，实际 {:?}",
                apply.default
            );
        }
    }
}
