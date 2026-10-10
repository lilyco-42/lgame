//! 驱动 —— 中间层的第三块地基：把动作落到某个具体目标上。
//!
//! # 为什么是 trait
//!
//! 「目标」是唯一与平台绑定的东西。屏幕档位（截屏 + 注入键鼠）只是**一个**
//! 实现。将来接机器人时新增一个 `RosDriver` / `SerialDriver`，
//! [`crate::session`] 与 [`crate::policy`] 一行都不用改。
//!
//! 驱动不负责「想」，只负责「看」和「做」——决策在策略层。

use std::path::Path;

use crate::action::Action;
use crate::embodiment::Embodiment;
use crate::error::Result;
use crate::observation::Observation;

/// 一个可被 AI 驱动的目标
pub trait Driver: Send {
    /// 目标标识（窗口标题 / 设备名）
    fn target(&self) -> &str;

    /// 该目标的躯体档位
    fn embodiment(&self) -> &Embodiment;

    /// 看一帧。`out_dir` 用于落盘画面（纯结构化躯体可忽略）
    fn observe(&mut self, out_dir: &Path) -> Result<Observation>;

    /// 做一个动作。
    ///
    /// 返回**警告列表**（不是错误）：例如「2D 躯体不支持 crouch，已忽略」。
    /// 这类情况不该中断循环，但必须让调用方看得见 —— 否则「跳了没反应」
    /// 会变成查不出来的玄学。
    fn execute(&mut self, action: &Action) -> Result<Vec<String>>;
}

// ── 屏幕档位（截屏 + 键鼠注入） ─────────────────────────────

#[cfg(feature = "screen")]
mod screen {
    use std::path::Path;
    use std::thread::sleep;
    use std::time::Duration;

    use super::{Action, Driver, Embodiment, Observation, Result};
    use crate::action::Input;
    use crate::capture;
    use crate::embodiment::LookMode;
    use crate::error::GameError;
    use crate::input::Injector;

    /// 通过「看屏幕 + 注入键鼠」驱动一个窗口里的游戏
    pub struct ScreenDriver {
        window: String,
        embodiment: Embodiment,
        tick: u64,
        max_width: Option<u32>,
        injector: Injector,
    }

    impl ScreenDriver {
        pub fn new(window: &str, embodiment: Embodiment, max_width: Option<u32>) -> Result<Self> {
            if window.trim().is_empty() {
                return Err(GameError::Config(
                    "必须给一个窗口标题匹配串（用 `doctor` 看当前可见窗口）".into(),
                ));
            }
            Ok(ScreenDriver {
                window: window.to_string(),
                embodiment,
                tick: 0,
                max_width,
                injector: Injector::new()?,
            })
        }

        /// 画面最大宽度（等比缩放后再送 LLM，省 token）
        pub fn max_width(&self) -> Option<u32> {
            self.max_width
        }
    }

    fn push_unique(v: &mut Vec<Input>, i: Input) {
        if !v.contains(&i) {
            v.push(i);
        }
    }

    impl Driver for ScreenDriver {
        fn target(&self) -> &str {
            &self.window
        }

        fn embodiment(&self) -> &Embodiment {
            &self.embodiment
        }

        fn observe(&mut self, out_dir: &Path) -> Result<Observation> {
            self.tick += 1;
            let path: std::path::PathBuf = out_dir.join(format!("tick-{:05}.png", self.tick));
            let info = capture::capture_window(&self.window, &path, self.max_width)?;
            Ok(Observation::new(self.tick, info.title).with_frame(path))
        }

        fn execute(&mut self, action: &Action) -> Result<Vec<String>> {
            let a = action.clone().clamped();
            let em = self.embodiment.clone();
            let mut warnings: Vec<String> = Vec::new();

            // 1) 移动：模拟量先按躯体量化
            let f = em.quantize(a.forward);
            let s = em.quantize(a.strafe);
            let mut held: Vec<Input> = Vec::new();
            if f > 0.0 {
                push_unique(&mut held, em.keymap.forward);
            } else if f < 0.0 {
                push_unique(&mut held, em.keymap.backward);
            }
            if s > 0.0 {
                push_unique(&mut held, em.keymap.right);
            } else if s < 0.0 {
                push_unique(&mut held, em.keymap.left);
            }

            // 2) 按住类动词
            for v in &a.hold {
                if em.supports(*v) {
                    push_unique(&mut held, v.to_input(&em.keymap));
                } else {
                    warnings.push(format!(
                        "躯体 `{}` 不支持动词 `{}`，已忽略",
                        em.id,
                        v.as_str()
                    ));
                }
            }

            for i in &held {
                self.injector.hold(*i)?;
            }

            // 3) 点按类动词
            for v in &a.tap {
                if em.supports(*v) {
                    self.injector.tap(v.to_input(&em.keymap))?;
                } else {
                    warnings.push(format!(
                        "躯体 `{}` 不支持动词 `{}`，已忽略",
                        em.id,
                        v.as_str()
                    ));
                }
            }

            // 4) 视线 + 等待
            let wants_look = a.yaw != 0.0 || a.pitch != 0.0;
            if wants_look && em.look != LookMode::RelativeMouse {
                warnings.push(format!(
                    "躯体 `{}` 没有独立视线轴（look={:?}），yaw/pitch 已忽略",
                    em.id, em.look
                ));
            }

            if em.look == LookMode::RelativeMouse && wants_look {
                // 把总位移切成每 ~16ms 一步注入：一次性推几千像素会被游戏丢帧
                let steps = (a.duration_ms / 16).max(1);
                let total_dx = (a.yaw * em.px_per_deg).round() as i32;
                let total_dy = (a.pitch * em.px_per_deg).round() as i32;
                let n = steps as i32;
                let dx_step = total_dx / n;
                let dy_step = total_dy / n;
                // 余数放到第一步，避免累计误差
                let dx_rest = total_dx - dx_step * n;
                let dy_rest = total_dy - dy_step * n;
                let step_ms = a.duration_ms / steps;

                for k in 0..steps {
                    let extra_x = if k == 0 { dx_rest } else { 0 };
                    let extra_y = if k == 0 { dy_rest } else { 0 };
                    self.injector.look(dx_step + extra_x, dy_step + extra_y)?;
                    if step_ms > 0 {
                        sleep(Duration::from_millis(step_ms));
                    }
                }
                let rest = a.duration_ms.saturating_sub(step_ms * steps);
                if rest > 0 {
                    sleep(Duration::from_millis(rest));
                }
            } else if a.duration_ms > 0 {
                sleep(Duration::from_millis(a.duration_ms));
            }

            // 5) 说话：开聊天框 → 打文本 → 回车
            if let Some(text) = &a.say {
                if em.supports(crate::action::Verb::Chat) {
                    self.injector.tap(em.keymap.chat)?;
                    sleep(Duration::from_millis(80));
                    self.injector.text(text)?;
                    sleep(Duration::from_millis(60));
                    self.injector.tap(em.keymap.confirm)?;
                } else {
                    warnings.push(format!("躯体 `{}` 不支持聊天，say 已忽略", em.id));
                }
            }

            // 6) 释放所有按住的键（即使中途出错也要尽量释放）
            for i in &held {
                self.injector.release(*i)?;
            }

            Ok(warnings)
        }
    }
}

#[cfg(feature = "screen")]
pub use screen::ScreenDriver;
