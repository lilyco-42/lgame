//! 躯体档位 —— 中间层的第二块地基。
//!
//! 同一组 [`crate::action::Action`]，在不同躯体上落地方式不同。
//! 「躯体」描述的是**能力**而不是具体游戏：有没有独立视线轴、移动是连续量
//! 还是八方向、每度偏航对应多少鼠标像素。
//!
//! 目前内置三档：
//! - [`Embodiment::three_d`] 第一/第三人称 3D（Minecraft、FPS、TPS）
//! - [`Embodiment::two_d`] 2D 平台跳跃 / 俯视（Alice in Cradle、自走棋）
//! - [`Embodiment::robot`] 机器人底盘 —— **占位**，证明这套抽象不绑游戏
//!
//! 将来接机器人时不需要改 [`crate::action::Action`]，只需要：
//! 1. 新建一档 `Embodiment`（`look = LookMode::Servo`、`locomotion = Locomotion::Velocity`）
//! 2. 实现一个 `Driver` 把 `forward/strafe/yaw` 转成底盘线速度与云台角速度
//! 3. 策略层（LLM 提示词）复用同一套动作语义

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::action::{Keymap, Verb};

/// 视线轴模式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LookMode {
    /// 没有独立视线轴（2D 平台跳跃、俯视自走棋）
    None,
    /// 相对鼠标位移控制视角（3D 第一/第三人称）
    RelativeMouse,
    /// 云台伺服（机器人 —— 占位，尚未实现驱动）
    Servo,
}

/// 移动量模式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Locomotion {
    /// 连续量：`forward`/`strafe` 取任意 `[-1,1]`（摇杆 / 键盘组合）
    Analog,
    /// 八方向数字键：`[-1,1]` 被量化成 `-1/0/1`
    Digital8,
    /// 速度指令（机器人底盘 —— 占位）
    Velocity,
}

/// 一档躯体
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Embodiment {
    /// 稳定标识：`3d` / `2d` / `robot`
    pub id: String,
    /// 人类可读标题
    pub title: String,
    pub look: LookMode,
    pub locomotion: Locomotion,
    /// 每 1 度偏航对应多少鼠标像素（3D 档才有意义）
    pub px_per_deg: f32,
    /// 该躯体支持的离散动作（决定提示词里列出哪些动词）
    pub verbs: Vec<Verb>,
    pub keymap: Keymap,
}

impl Embodiment {
    /// 第一/第三人称 3D
    pub fn three_d() -> Self {
        Embodiment {
            id: "3d".into(),
            title: "第一/第三人称 3D".into(),
            look: LookMode::RelativeMouse,
            locomotion: Locomotion::Analog,
            // 游戏里常见的灵敏度：转 90° 大约需要移动 450 像素
            px_per_deg: 5.0,
            verbs: vec![
                Verb::Jump,
                Verb::Crouch,
                Verb::Sprint,
                Verb::Walk,
                Verb::Interact,
                Verb::Attack,
                Verb::AltAttack,
                Verb::Reload,
                Verb::Aim,
                Verb::Inventory,
                Verb::Pause,
                Verb::Confirm,
                Verb::Cancel,
                Verb::Chat,
            ],
            keymap: Keymap::desktop_3d(),
        }
    }

    /// 2D 平台跳跃 / 俯视
    pub fn two_d() -> Self {
        Embodiment {
            id: "2d".into(),
            title: "2D 平台跳跃 / 俯视".into(),
            look: LookMode::None,
            locomotion: Locomotion::Digital8,
            px_per_deg: 0.0,
            verbs: vec![
                Verb::Jump,
                Verb::Crouch,
                Verb::Interact,
                Verb::Attack,
                Verb::AltAttack,
                Verb::Dodge,
                Verb::Inventory,
                Verb::Map,
                Verb::Pause,
                Verb::Confirm,
                Verb::Cancel,
            ],
            keymap: Keymap::desktop_2d(),
        }
    }

    /// 机器人底盘 —— 占位档位。
    ///
    /// 刻意保留在这里而不是等将来再加：它证明 [`crate::action::Action`]
    /// 的动作空间**不是**为游戏设计的 —— `forward/strafe/yaw/pitch` 本来就
    /// 是底盘与云台的自由度。接真机时把 `keymap` 换成能力名即可。
    pub fn robot() -> Self {
        Embodiment {
            id: "robot".into(),
            title: "机器人底盘 + 云台（占位）".into(),
            look: LookMode::Servo,
            locomotion: Locomotion::Velocity,
            px_per_deg: 0.0,
            verbs: vec![Verb::Pause],
            keymap: Keymap::desktop_3d(),
        }
    }

    /// 按 id 取档位
    pub fn by_id(id: &str) -> Option<Self> {
        match id.trim().to_ascii_lowercase().as_str() {
            "3d" | "3" | "fps" | "tps" | "first-person" => Some(Embodiment::three_d()),
            "2d" | "2" | "platformer" | "topdown" => Some(Embodiment::two_d()),
            "robot" => Some(Embodiment::robot()),
            _ => None,
        }
    }

    /// 全部内置档位
    pub fn all() -> Vec<Self> {
        vec![
            Embodiment::three_d(),
            Embodiment::two_d(),
            Embodiment::robot(),
        ]
    }

    /// 把 `[-1,1]` 的连续量按本档位量化
    ///
    /// - `Analog`：原样（带 0.05 死区，避免摇杆漂移）
    /// - `Digital8` / `Velocity`：量化到 `-1/0/1`
    pub fn quantize(&self, v: f32) -> f32 {
        if v.is_nan() || v.abs() < 0.05 {
            return 0.0;
        }
        match self.locomotion {
            Locomotion::Analog => v.clamp(-1.0, 1.0),
            Locomotion::Digital8 | Locomotion::Velocity => {
                if v > 0.0 {
                    1.0
                } else {
                    -1.0
                }
            }
        }
    }

    /// 本档位是否支持某个动词
    pub fn supports(&self, v: Verb) -> bool {
        self.verbs.contains(&v)
    }

    /// 把该档位支持的动词列成提示词片段，如 `jump, crouch, attack`
    pub fn verb_list(&self) -> String {
        self.verbs
            .iter()
            .map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

// ── 默认档位（供 l3d / l2d 在 main 里设定） ─────────────────

static DEFAULT_ID: OnceLock<String> = OnceLock::new();

/// 设定本进程的默认躯体档位（`l3d` → `"3d"`，`l2d` → `"2d"`）。
///
/// 只允许设置一次；重复调用被忽略（第一个赢）。
pub fn set_default(id: &str) {
    let _ = DEFAULT_ID.set(id.to_string());
}

/// 默认档位的 id（未设定时为 `"3d"`）
pub fn default_id() -> &'static str {
    DEFAULT_ID.get().map(|s| s.as_str()).unwrap_or("3d")
}

/// 默认档位本体
pub fn default_embodiment() -> Embodiment {
    Embodiment::by_id(default_id()).unwrap_or_else(Embodiment::three_d)
}

/// 解析 `auto`：`auto`/空 → 默认档位；否则按 id 解析，失败给可读错误
pub fn resolve(spec: &str) -> crate::error::Result<Embodiment> {
    let s = spec.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("auto") {
        return Ok(default_embodiment());
    }
    Embodiment::by_id(s).ok_or_else(|| {
        crate::error::GameError::Config(format!(
            "未知躯体档位 `{s}`；可选：3d / 2d / robot（或用 auto 取本二进制默认档位）"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_d_has_a_look_axis_and_2d_does_not() {
        assert_eq!(Embodiment::three_d().look, LookMode::RelativeMouse);
        assert_eq!(Embodiment::two_d().look, LookMode::None);
    }

    #[test]
    fn quantize_snaps_for_digital_locomotion() {
        let two = Embodiment::two_d();
        assert_eq!(two.quantize(0.3), 1.0);
        assert_eq!(two.quantize(-0.9), -1.0);
        assert_eq!(two.quantize(0.01), 0.0); // 死区
        let three = Embodiment::three_d();
        assert_eq!(three.quantize(0.3), 0.3); // 模拟量原样保留
    }

    #[test]
    fn by_id_accepts_aliases_and_rejects_junk() {
        assert_eq!(Embodiment::by_id("3D").unwrap().id, "3d");
        assert_eq!(Embodiment::by_id("fps").unwrap().id, "3d");
        assert_eq!(Embodiment::by_id("platformer").unwrap().id, "2d");
        assert_eq!(Embodiment::by_id("robot").unwrap().id, "robot");
        assert!(Embodiment::by_id("nintendo-switch").is_none());
    }

    #[test]
    fn resolve_auto_falls_back_to_default() {
        let e = resolve("auto").unwrap();
        assert_eq!(e.id, default_id());
        assert!(resolve("").is_ok());
        let err = resolve("gamecube").unwrap_err();
        assert!(err.to_string().contains("未知躯体档位"), "{err}");
    }

    #[test]
    fn robot_profile_exists_to_prove_the_abstraction_is_not_game_specific() {
        let r = Embodiment::robot();
        assert_eq!(r.look, LookMode::Servo);
        assert_eq!(r.locomotion, Locomotion::Velocity);
    }

    #[test]
    fn verb_list_is_comma_separated_names() {
        let s = Embodiment::two_d().verb_list();
        assert!(s.contains("jump"), "{s}");
        assert!(s.contains("attack"), "{s}");
        assert!(!s.contains("jump,") || s.contains(", "), "{s}");
    }
}
