//! 动作空间 —— 中间层的第一块地基。
//!
//! # 为什么不是「按下 W 键」
//!
//! 因为这一层要同时服务三种躯体：
//!
//! | 躯体 | 前进 | 视线 | 攻击 |
//! |---|---|---|---|
//! | 3D 第一人称（Minecraft） | W 键按住 | 相对鼠标位移 | 鼠标左键 |
//! | 2D 平台跳跃（Alice in Cradle） | 方向键/摇杆 | 无独立视线轴 | 键盘 Z |
//! | 机器人底盘（将来） | 线速度 m/s | 云台角速度 | 机械臂指令 |
//!
//! 如果动作里写死「按 W」，第三种躯体就没法接。所以这里只描述**意图**：
//! 往前多少、往右多少、视角转多少度、按住哪些语义动作、持续多久。
//! 由 [`crate::embodiment::Embodiment`] 负责把意图翻译成具体物理输入。

use serde::{Deserialize, Serialize};

/// 鼠标按键（`attack` 在 3D 游戏里通常是左键而不是某个键）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseBtn {
    Left,
    Right,
    Middle,
}

/// 一条物理输入：要么是键盘扫描码，要么是鼠标键。
///
/// 键盘用**扫描码**而不是字符，因为游戏读的是物理键位 ——
/// enigo 官方文档明确建议：游戏里按 WASD 要用 `Keyboard::raw`，
/// 否则在非 QWERTY 布局下 `'w'` 会映射到别的物理键。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Input {
    /// 物理扫描码（PS/2 Set 1）
    Key(u16),
    Mouse(MouseBtn),
}

impl Input {
    /// 便捷构造
    pub const fn key(scancode: u16) -> Self {
        Input::Key(scancode)
    }
}

/// 离散语义动作 —— 与键位无关的「动词」
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verb {
    /// 跳
    Jump,
    /// 下蹲 / 蹲伏
    Crouch,
    /// 疾跑
    Sprint,
    /// 慢走（潜行）
    Walk,
    /// 交互 / 拾取 / 开门 / 对话确认
    Interact,
    /// 主攻击
    Attack,
    /// 副攻击 / 瞄准
    AltAttack,
    /// 换弹 / 充能
    Reload,
    /// 闪避 / 翻滚
    Dodge,
    /// 举枪瞄准（3D 射击类）
    Aim,
    /// 背包
    Inventory,
    /// 地图
    Map,
    /// 暂停
    Pause,
    /// 确认 / 提交（菜单）
    Confirm,
    /// 取消 / 返回（菜单）
    Cancel,
    /// 打开聊天框
    Chat,
}

impl Verb {
    /// 这个动词在该躯体下映射到哪条物理输入
    pub fn to_input(self, keys: &Keymap) -> Input {
        match self {
            Verb::Jump => keys.jump,
            Verb::Crouch => keys.crouch,
            Verb::Sprint => keys.sprint,
            Verb::Walk => keys.walk,
            Verb::Interact => keys.interact,
            Verb::Attack => keys.attack,
            Verb::AltAttack => keys.alt_attack,
            Verb::Reload => keys.reload,
            Verb::Dodge => keys.dodge,
            Verb::Aim => keys.aim,
            Verb::Inventory => keys.inventory,
            Verb::Map => keys.map,
            Verb::Pause => keys.pause,
            Verb::Confirm => keys.confirm,
            Verb::Cancel => keys.cancel,
            Verb::Chat => keys.chat,
        }
    }

    /// 全部动词（用于生成给 LLM 的动作说明）
    pub const ALL: &'static [Verb] = &[
        Verb::Jump,
        Verb::Crouch,
        Verb::Sprint,
        Verb::Walk,
        Verb::Interact,
        Verb::Attack,
        Verb::AltAttack,
        Verb::Reload,
        Verb::Dodge,
        Verb::Aim,
        Verb::Inventory,
        Verb::Map,
        Verb::Pause,
        Verb::Confirm,
        Verb::Cancel,
        Verb::Chat,
    ];

    /// 稳定的小写下划线名（与 serde 的 rename_all 一致，用于提示词与解析）
    pub fn as_str(self) -> &'static str {
        match self {
            Verb::Jump => "jump",
            Verb::Crouch => "crouch",
            Verb::Sprint => "sprint",
            Verb::Walk => "walk",
            Verb::Interact => "interact",
            Verb::Attack => "attack",
            Verb::AltAttack => "alt_attack",
            Verb::Reload => "reload",
            Verb::Dodge => "dodge",
            Verb::Aim => "aim",
            Verb::Inventory => "inventory",
            Verb::Map => "map",
            Verb::Pause => "pause",
            Verb::Confirm => "confirm",
            Verb::Cancel => "cancel",
            Verb::Chat => "chat",
        }
    }

    /// 从字符串解析（大小写不敏感，允许连字符）
    pub fn parse(s: &str) -> Option<Verb> {
        let norm = s.trim().to_ascii_lowercase().replace('-', "_");
        Verb::ALL.iter().copied().find(|v| v.as_str() == norm)
    }
}

/// 一个动作 —— 中间层对外唯一的「输出」形状。
///
/// 所有字段都有默认值（全零 / 空 / `None`），所以 LLM 只写它关心的字段即可
/// （`{"forward": 1.0, "duration_ms": 500}` 是合法动作）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Action {
    /// 前后：`+1` 向前，`-1` 向后。会被钳到 `[-1, 1]`
    pub forward: f32,
    /// 左右平移：`+1` 向右，`-1` 向左
    pub strafe: f32,
    /// 视角偏航增量（度）：`+` 向右转。无视线轴的躯体忽略
    pub yaw: f32,
    /// 视角俯仰增量（度）：`+` 抬头。无视线轴的躯体忽略
    pub pitch: f32,
    /// 持续时长（毫秒）：期间保持移动/视线转向
    pub duration_ms: u64,
    /// 期间**按住**的动词（如 Sprint、Aim）
    pub hold: Vec<Verb>,
    /// 期间**点按一次**的动词（如 Jump、Attack）
    pub tap: Vec<Verb>,
    /// 说话（走文本注入，不走扫描码）
    pub say: Option<String>,
    /// 给策略自述「为什么这么做」，不参与执行（只进 trace）
    pub note: Option<String>,
}

impl Action {
    /// 什么都不做，只等待
    pub fn wait(ms: u64) -> Self {
        Action {
            duration_ms: ms,
            ..Default::default()
        }
    }

    /// 是否包含任何会改变世界的量（用于 dry-run 判断与日志）
    pub fn is_noop(&self) -> bool {
        self.forward == 0.0
            && self.strafe == 0.0
            && self.yaw == 0.0
            && self.pitch == 0.0
            && self.hold.is_empty()
            && self.tap.is_empty()
            && self.say.is_none()
    }

    /// 钳到合法范围，避免 LLM 给出 `forward: 999` 这种值
    pub fn clamped(mut self) -> Self {
        self.forward = clamp_unit(self.forward);
        self.strafe = clamp_unit(self.strafe);
        self.yaw = clamp_deg(self.yaw);
        self.pitch = clamp_deg(self.pitch);
        self.duration_ms = self.duration_ms.min(60_000);
        self
    }

    /// 人类可读的一行摘要（trace / 日志用）
    pub fn summary(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.forward != 0.0 {
            parts.push(format!("前后{:+.2}", self.forward));
        }
        if self.strafe != 0.0 {
            parts.push(format!("左右{:+.2}", self.strafe));
        }
        if self.yaw != 0.0 || self.pitch != 0.0 {
            parts.push(format!("视角{:+.0}°/{:+.0}°", self.yaw, self.pitch));
        }
        if !self.hold.is_empty() {
            let v: Vec<&str> = self.hold.iter().map(|x| x.as_str()).collect();
            parts.push(format!("按住[{}]", v.join(",")));
        }
        if !self.tap.is_empty() {
            let v: Vec<&str> = self.tap.iter().map(|x| x.as_str()).collect();
            parts.push(format!("点按[{}]", v.join(",")));
        }
        if let Some(s) = &self.say {
            parts.push(format!("说“{s}”"));
        }
        if parts.is_empty() {
            parts.push(format!("静止 {}ms", self.duration_ms));
        } else if self.duration_ms > 0 {
            parts.push(format!("{}ms", self.duration_ms));
        }
        parts.join(" ")
    }
}

fn clamp_unit(v: f32) -> f32 {
    if v.is_nan() {
        0.0
    } else {
        v.clamp(-1.0, 1.0)
    }
}

fn clamp_deg(v: f32) -> f32 {
    if v.is_nan() {
        0.0
    } else {
        v.clamp(-360.0, 360.0)
    }
}

/// 物理扫描码表（PS/2 Set 1）。
///
/// # 为什么是扫描码而不是 `enigo::Key`
///
/// 实测 enigo 0.6.1：`Key::A`..`Key::Z`、`Key::Num0`..`Num9` 全部带
/// `#[cfg(target_os = "windows")]` —— 是 Windows 独占变体，跨平台用不了。
/// 而 `Keyboard::raw(u16)` 在所有平台都有，且 enigo 官方文档明确建议
/// 「游戏里按 WASD 用 raw，避免布局/键位映射干扰」。
///
/// # 为什么没有方向键
///
/// 方向键属于**扩展键**，需要平台相关的扩展位（enigo 的 `EXT` 常量本身也是
/// `#[cfg(target_os = "windows")]`）。为了不把一个平台相关的魔数埋进跨平台代码里，
/// 2D 档位改用 WASD（同样能表达八方向：`W`/`WA`/`A`/`AS`/`S`/`SD`/`D`/`DW`）。
/// 需要方向键的键位表可以自行加，但必须按平台分别处理扩展位。
///
/// 只列游戏里真正会按到的键，避免变成一个「键盘驱动」项目。
pub mod sc {
    pub const ESC: u16 = 0x01;
    pub const BACKSPACE: u16 = 0x0E;
    pub const TAB: u16 = 0x0F;
    pub const ENTER: u16 = 0x1C;
    pub const CTRL: u16 = 0x1D;
    pub const LSHIFT: u16 = 0x2A;
    pub const LALT: u16 = 0x38;
    pub const SPACE: u16 = 0x39;

    pub const Q: u16 = 0x10;
    pub const W: u16 = 0x11;
    pub const E: u16 = 0x12;
    pub const R: u16 = 0x13;
    pub const T: u16 = 0x14;
    pub const Y: u16 = 0x15;
    pub const U: u16 = 0x16;
    pub const I: u16 = 0x17;
    pub const O: u16 = 0x18;
    pub const P: u16 = 0x19;
    pub const A: u16 = 0x1E;
    pub const S: u16 = 0x1F;
    pub const D: u16 = 0x20;
    pub const F: u16 = 0x21;
    pub const G: u16 = 0x22;
    pub const H: u16 = 0x23;
    pub const J: u16 = 0x24;
    pub const K: u16 = 0x25;
    pub const L: u16 = 0x26;
    pub const Z: u16 = 0x2C;
    pub const X: u16 = 0x2D;
    pub const C: u16 = 0x2E;
    pub const V: u16 = 0x2F;
    pub const B: u16 = 0x30;
    pub const N: u16 = 0x31;
    pub const M: u16 = 0x32;

    /// 数字行 1..9 的扫描码（下标 0 对应 '1'）
    pub const DIGITS: [u16; 9] = [0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A];
}

/// 键位表 —— 躯体的一部分。换游戏 = 换一张表，动作空间不变。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Keymap {
    pub forward: Input,
    pub backward: Input,
    pub left: Input,
    pub right: Input,
    pub jump: Input,
    pub crouch: Input,
    pub sprint: Input,
    pub walk: Input,
    pub interact: Input,
    pub attack: Input,
    pub alt_attack: Input,
    pub reload: Input,
    pub dodge: Input,
    pub aim: Input,
    pub inventory: Input,
    pub map: Input,
    pub pause: Input,
    pub confirm: Input,
    pub cancel: Input,
    pub chat: Input,
    /// 快捷栏 1..9
    pub hotbar: [Input; 9],
}

impl Keymap {
    /// 桌面 3D 射击 / 沙盒的通用布局（WASD + 鼠标）
    pub fn desktop_3d() -> Self {
        Keymap {
            forward: Input::key(sc::W),
            backward: Input::key(sc::S),
            left: Input::key(sc::A),
            right: Input::key(sc::D),
            jump: Input::key(sc::SPACE),
            crouch: Input::key(sc::CTRL),
            sprint: Input::key(sc::LSHIFT),
            walk: Input::key(sc::LALT),
            interact: Input::key(sc::E),
            attack: Input::Mouse(MouseBtn::Left),
            alt_attack: Input::Mouse(MouseBtn::Right),
            reload: Input::key(sc::R),
            dodge: Input::key(sc::V),
            aim: Input::Mouse(MouseBtn::Right),
            inventory: Input::key(sc::E),
            map: Input::key(sc::M),
            pause: Input::key(sc::ESC),
            confirm: Input::key(sc::ENTER),
            cancel: Input::key(sc::ESC),
            chat: Input::key(sc::T),
            hotbar: [
                Input::key(sc::DIGITS[0]),
                Input::key(sc::DIGITS[1]),
                Input::key(sc::DIGITS[2]),
                Input::key(sc::DIGITS[3]),
                Input::key(sc::DIGITS[4]),
                Input::key(sc::DIGITS[5]),
                Input::key(sc::DIGITS[6]),
                Input::key(sc::DIGITS[7]),
                Input::key(sc::DIGITS[8]),
            ],
        }
    }

    /// 2D 平台跳跃的通用布局（WASD 八方向 + Z/X/C）
    ///
    /// 用 WASD 而不是方向键，见 [`sc`] 的说明（扩展键要平台相关的扩展位）。
    pub fn desktop_2d() -> Self {
        Keymap {
            forward: Input::key(sc::W),
            backward: Input::key(sc::S),
            left: Input::key(sc::A),
            right: Input::key(sc::D),
            jump: Input::key(sc::Z),
            crouch: Input::key(sc::CTRL),
            sprint: Input::key(sc::LSHIFT),
            walk: Input::key(sc::LALT),
            interact: Input::key(sc::E),
            attack: Input::key(sc::X),
            alt_attack: Input::key(sc::C),
            reload: Input::key(sc::R),
            dodge: Input::key(sc::LSHIFT),
            aim: Input::key(sc::V),
            inventory: Input::key(sc::I),
            map: Input::key(sc::M),
            pause: Input::key(sc::ESC),
            confirm: Input::key(sc::Z),
            cancel: Input::key(sc::X),
            chat: Input::key(sc::T),
            hotbar: [
                Input::key(sc::DIGITS[0]),
                Input::key(sc::DIGITS[1]),
                Input::key(sc::DIGITS[2]),
                Input::key(sc::DIGITS[3]),
                Input::key(sc::DIGITS[4]),
                Input::key(sc::DIGITS[5]),
                Input::key(sc::DIGITS[6]),
                Input::key(sc::DIGITS[7]),
                Input::key(sc::DIGITS[8]),
            ],
        }
    }
}

impl Default for Keymap {
    fn default() -> Self {
        Keymap::desktop_3d()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_action_is_noop_and_parses_from_empty_json() {
        let a: Action = serde_json::from_str("{}").unwrap();
        assert_eq!(a, Action::default());
        assert!(a.is_noop());
    }

    #[test]
    fn partial_json_fills_defaults() {
        let a: Action = serde_json::from_str(r#"{"forward":1.0,"duration_ms":500}"#).unwrap();
        assert_eq!(a.forward, 1.0);
        assert_eq!(a.duration_ms, 500);
        assert_eq!(a.strafe, 0.0);
        assert!(a.tap.is_empty());
    }

    #[test]
    fn verbs_roundtrip_through_str() {
        for v in Verb::ALL {
            assert_eq!(Verb::parse(v.as_str()), Some(*v));
        }
        assert_eq!(Verb::parse("Alt-Attack"), Some(Verb::AltAttack));
        assert_eq!(Verb::parse("  JUMP "), Some(Verb::Jump));
        assert_eq!(Verb::parse("teleport"), None);
    }

    #[test]
    fn verb_serializes_snake_case() {
        let j = serde_json::to_string(&Verb::AltAttack).unwrap();
        assert_eq!(j, "\"alt_attack\"");
        let back: Verb = serde_json::from_str(&j).unwrap();
        assert_eq!(back, Verb::AltAttack);
    }

    #[test]
    fn clamping_keeps_values_sane() {
        let a = Action {
            forward: 999.0,
            strafe: -5.0,
            yaw: 10_000.0,
            pitch: f32::NAN,
            duration_ms: 10_000_000,
            ..Default::default()
        }
        .clamped();
        assert_eq!(a.forward, 1.0);
        assert_eq!(a.strafe, -1.0);
        assert_eq!(a.yaw, 360.0);
        assert_eq!(a.pitch, 0.0);
        assert_eq!(a.duration_ms, 60_000);
    }

    #[test]
    fn keymaps_map_every_verb_to_something() {
        for keys in [Keymap::desktop_3d(), Keymap::desktop_2d()] {
            for v in Verb::ALL {
                // 只要求「有映射」，不要求互不相同（2D 里上下键兼任交互是合理的）
                let _ = v.to_input(&keys);
            }
        }
    }

    #[test]
    fn attack_is_a_mouse_button_in_3d() {
        let k = Keymap::desktop_3d();
        assert_eq!(k.attack, Input::Mouse(MouseBtn::Left));
        assert_eq!(k.forward, Input::key(sc::W));
    }

    #[test]
    fn summary_is_human_readable() {
        let a = Action {
            forward: 1.0,
            yaw: -90.0,
            tap: vec![Verb::Jump],
            duration_ms: 400,
            ..Default::default()
        };
        let s = a.summary();
        assert!(s.contains("前后+1.00"), "{s}");
        assert!(s.contains("视角-90°"), "{s}");
        assert!(s.contains("点按[jump]"), "{s}");
        assert!(s.contains("400ms"), "{s}");
    }
}
