//! 目标档位 —— 「哪个游戏 + 哪档躯体 + 怎么找它的窗口」。
//!
//! 这一层是**纯数据**：换游戏不改一行代码，加一条记录即可。
//! 内置条目优先选**你拥有源码的游戏**（能加原生桥、能改键位），
//! 其次是通用的商业游戏（只能靠屏幕档位）。

use serde::{Deserialize, Serialize};

use crate::embodiment::Embodiment;

/// 一个可驱动的目标
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Target {
    /// 稳定标识，CLI 里用 `--target aic`
    pub id: String,
    /// 人类可读名
    pub title: String,
    /// 窗口标题匹配串（大小写不敏感子串）
    pub window: String,
    /// 躯体档位 id：`3d` / `2d`
    pub embodiment: String,
    /// 为什么推荐它 / 注意什么
    pub note: String,
}

impl Target {
    /// 解析出该目标对应的躯体档位
    pub fn embodiment(&self) -> Option<Embodiment> {
        Embodiment::by_id(&self.embodiment)
    }
}

/// 内置目标表
///
/// 选型原则（对应「先找游戏」这一步）：
/// 1. **优先自有源码的游戏** —— 能加原生桥、能改键位、能确定性重放；
/// 2. 覆盖两种躯体（3D 第一人称 / 2D 平台跳跃），证明中间层不绑单一形态；
/// 3. 都必须是本机能跑起来的，不推荐「理论上支持」的游戏。
pub fn builtin() -> Vec<Target> {
    vec![
        // ── 3D ────────────────────────────────────────────
        Target {
            id: "minecraft".into(),
            title: "Minecraft Java 版".into(),
            window: "Minecraft".into(),
            embodiment: "3d".into(),
            note: "第一人称 3D 的基准靶子：WASD + 鼠标视角 + 跳跃，动作空间最完整。\
                   生态里已有 fabric-mod-template / mc-server-controller，将来可加引擎级原生桥。"
                .into(),
        },
        Target {
            id: "zombie-raid".into(),
            title: "zombie-raid（自有 Godot 4.7）".into(),
            window: "zombie-raid".into(),
            embodiment: "3d".into(),
            note: "自有源码的 3D 第一人称 PVE。Godot 的远程调试协议将来可做原生桥，\
                   拿到真实坐标而不是只靠画面。"
                .into(),
        },
        // ── 2D ────────────────────────────────────────────
        Target {
            id: "aic".into(),
            title: "Alice in Cradle".into(),
            window: "AliceInCradle".into(),
            embodiment: "2d".into(),
            note: "2D 动作平台的基准靶子。Unity/Mono，已有 BepInEx 工具链（aic-android / lilyco-aic），\
                   将来可注入插件直接读角色坐标与状态。"
                .into(),
        },
        Target {
            id: "stronghold".into(),
            title: "卫戍协议：盟约（自有浏览器 2D）".into(),
            window: "卫戍协议".into(),
            embodiment: "2d".into(),
            note: "自有源码的浏览器 2D 自走棋。浏览器目标将来可走 CDP 注入，\
                   直接调用游戏内部 API 而不是模拟按键。"
                .into(),
        },
        Target {
            id: "tsukishita".into(),
            title: "tsukishita（自有浏览器游戏）".into(),
            window: "tsukishita".into(),
            embodiment: "2d".into(),
            note: "自有源码的浏览器 2D 游戏，适合做低风险实验。".into(),
        },
    ]
}

/// 按 id 查（大小写不敏感）
pub fn find(id: &str) -> Option<Target> {
    let key = id.trim().to_ascii_lowercase();
    builtin().into_iter().find(|t| t.id == key)
}

/// 按窗口标题猜目标（用于 `--window` 没给 target 时给个提示）
pub fn guess_by_window(title: &str) -> Option<Target> {
    builtin()
        .into_iter()
        .find(|t| crate::util::title_matches(&t.window, title))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_targets_are_well_formed() {
        let all = builtin();
        assert!(all.len() >= 5, "{}", all.len());
        for t in &all {
            assert!(!t.id.is_empty(), "{t:?}");
            assert!(!t.window.is_empty(), "{} 缺窗口匹配串", t.id);
            assert!(
                t.embodiment().is_some(),
                "{} 的躯体档位 `{}` 不存在",
                t.id,
                t.embodiment
            );
            assert!(!t.note.is_empty(), "{} 缺说明", t.id);
        }
    }

    #[test]
    fn covers_both_3d_and_2d() {
        let all = builtin();
        assert!(all.iter().any(|t| t.embodiment == "3d"));
        assert!(all.iter().any(|t| t.embodiment == "2d"));
    }

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<String> = builtin().into_iter().map(|t| t.id).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n, "目标 id 有重复");
    }

    #[test]
    fn find_is_case_insensitive() {
        assert_eq!(find("AIC").unwrap().id, "aic");
        assert_eq!(find(" Minecraft ").unwrap().embodiment, "3d");
        assert!(find("doom").is_none());
    }

    #[test]
    fn guess_by_window_matches_partial_title() {
        let t = guess_by_window("Minecraft 1.21.4 - 单人游戏").unwrap();
        assert_eq!(t.id, "minecraft");
        assert!(guess_by_window("记事本").is_none());
    }
}
