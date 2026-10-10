//! 输入注入档位：把 [`crate::action::Input`] 变成真实的键盘/鼠标事件。
//!
//! 用 `enigo`。全部走 `Keyboard::raw(scancode)` —— 见 [`crate::action::sc`]
//! 的说明：游戏读物理键位，`Key::Unicode('w')` 在非 QWERTY 布局下会按错键。

#![cfg(feature = "screen")]

use enigo::{Button, Coordinate, Direction, Enigo, Keyboard, Mouse, Settings};

use crate::action::{Input, MouseBtn};
use crate::error::{GameError, Result};

/// 输入注入器（持有 enigo 连接）
pub struct Injector {
    enigo: Enigo,
}

impl Injector {
    /// 建立连接。失败通常是权限问题：
    /// - macOS：需要「辅助功能」权限
    /// - Linux/Wayland：需要 uinput 权限
    /// - Windows：一般直接可用
    pub fn new() -> Result<Self> {
        let enigo = Enigo::new(&Settings::default())
            .map_err(|e| GameError::Input(format!("初始化输入注入失败（多半是权限）: {e}")))?;
        Ok(Injector { enigo })
    }

    /// 按下并保持
    pub fn hold(&mut self, input: Input) -> Result<()> {
        self.send(input, Direction::Press)
    }

    /// 松开
    pub fn release(&mut self, input: Input) -> Result<()> {
        self.send(input, Direction::Release)
    }

    /// 点按一次（按下 + 松开）
    pub fn tap(&mut self, input: Input) -> Result<()> {
        self.send(input, Direction::Click)
    }

    fn send(&mut self, input: Input, dir: Direction) -> Result<()> {
        match input {
            Input::Key(scancode) => self
                .enigo
                .raw(scancode, dir)
                .map_err(|e| GameError::Input(format!("按键 0x{scancode:04X} 失败: {e}"))),
            Input::Mouse(btn) => self
                .enigo
                .button(to_button(btn), dir)
                .map_err(|e| GameError::Input(format!("鼠标键失败: {e}"))),
        }
    }

    /// 相对移动鼠标（3D 视角）
    ///
    /// 正 `dx` 向右、正 `dy` 向下（enigo 的坐标系，见 `Mouse::move_mouse` 文档）。
    pub fn look(&mut self, dx: i32, dy: i32) -> Result<()> {
        if dx == 0 && dy == 0 {
            return Ok(());
        }
        self.enigo
            .move_mouse(dx, dy, Coordinate::Rel)
            .map_err(|e| GameError::Input(format!("鼠标移动失败: {e}")))
    }

    /// 注入文本（聊天 / 命名）。与键位表无关，走 enigo 的文本通道。
    pub fn text(&mut self, s: &str) -> Result<()> {
        if s.is_empty() {
            return Ok(());
        }
        self.enigo
            .text(s)
            .map_err(|e| GameError::Input(format!("文本注入失败: {e}")))
    }
}

fn to_button(btn: MouseBtn) -> Button {
    match btn {
        MouseBtn::Left => Button::Left,
        MouseBtn::Right => Button::Right,
        MouseBtn::Middle => Button::Middle,
    }
}
