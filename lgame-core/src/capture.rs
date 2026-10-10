//! 截屏档位：把「某个窗口的画面」变成一张 PNG。
//!
//! 用 `xcap`（跨平台，Windows/Linux(X11)/macOS）。Wayland 下窗口捕获不可用 ——
//! 这种情况返回明确的 [`GameError::Capture`]，而不是静默给一张黑图。

#![cfg(feature = "screen")]

use std::path::Path;

use crate::error::{GameError, Result};

/// 标题匹配（实现放在 [`crate::util`]，这里转出去方便调用方就近取用）
pub use crate::util::title_matches;

/// 窗口元信息
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WindowInfo {
    pub title: String,
    pub app: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub minimized: bool,
}

/// 列出所有可见窗口（不含最小化的），用于 `doctor` 与目标选择
pub fn list_windows() -> Result<Vec<WindowInfo>> {
    let windows =
        xcap::Window::all().map_err(|e| GameError::Capture(format!("枚举窗口失败: {e}")))?;
    let mut out = Vec::new();
    for w in windows {
        let minimized = w.is_minimized().unwrap_or(false);
        out.push(WindowInfo {
            title: w.title().unwrap_or_default(),
            app: w.app_name().unwrap_or_default(),
            x: w.x().unwrap_or(0),
            y: w.y().unwrap_or(0),
            width: w.width().unwrap_or(0),
            height: w.height().unwrap_or(0),
            minimized,
        });
    }
    Ok(out)
}

/// 抓取标题匹配的第一个窗口，存成 PNG
///
/// `max_width` 限制宽度（等比缩放）—— 直接送 4K 截图给 LLM 是浪费 token。
pub fn capture_window(pattern: &str, out: &Path, max_width: Option<u32>) -> Result<WindowInfo> {
    let windows =
        xcap::Window::all().map_err(|e| GameError::Capture(format!("枚举窗口失败: {e}")))?;

    let mut matched: Vec<WindowInfo> = Vec::new();
    for w in windows {
        let title = w.title().unwrap_or_default();
        if !title_matches(pattern, &title) {
            continue;
        }
        let info = WindowInfo {
            title: title.clone(),
            app: w.app_name().unwrap_or_default(),
            x: w.x().unwrap_or(0),
            y: w.y().unwrap_or(0),
            width: w.width().unwrap_or(0),
            height: w.height().unwrap_or(0),
            minimized: w.is_minimized().unwrap_or(false),
        };

        if info.minimized {
            matched.push(info);
            continue;
        }
        if info.width == 0 || info.height == 0 {
            matched.push(info);
            continue;
        }

        let image = w
            .capture_image()
            .map_err(|e| GameError::Capture(format!("捕获窗口 `{}` 失败: {e}", info.title)))?;
        let image = match max_width {
            Some(mw) if mw > 0 && image.width() > mw => {
                let nw = mw;
                let nh =
                    (image.height() as f64 * (mw as f64 / image.width() as f64)).round() as u32;
                image::imageops::resize(
                    &image,
                    nw,
                    nh.max(1),
                    image::imageops::FilterType::Triangle,
                )
            }
            _ => image,
        };
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        image
            .save(out)
            .map_err(|e| GameError::Capture(format!("写 PNG `{}` 失败: {e}", out.display())))?;
        return Ok(info);
    }

    if let Some(m) = matched.into_iter().next() {
        return Err(GameError::Target(format!(
            "窗口 `{}` 匹配到了但不可用（最小化或尺寸为 0）—— 请先把游戏窗口恢复出来",
            m.title
        )));
    }

    Err(GameError::Target(format!(
        "找不到标题包含 `{pattern}` 的窗口。用 `l3d doctor` 或 `l2d doctor` 看当前可见窗口列表"
    )))
}

/// 抓主显示器全屏（没有窗口标题可用时的兜底）
pub fn capture_primary_monitor(out: &Path, max_width: Option<u32>) -> Result<()> {
    let monitors =
        xcap::Monitor::all().map_err(|e| GameError::Capture(format!("枚举显示器失败: {e}")))?;
    let monitor = monitors
        .into_iter()
        .find(|m| m.is_primary().unwrap_or(false))
        .ok_or_else(|| GameError::Capture("找不到主显示器".to_string()))?;

    let image = monitor
        .capture_image()
        .map_err(|e| GameError::Capture(format!("捕获主显示器失败: {e}")))?;
    let image = match max_width {
        Some(mw) if mw > 0 && image.width() > mw => {
            let nh = (image.height() as f64 * (mw as f64 / image.width() as f64)).round() as u32;
            image::imageops::resize(&image, mw, nh.max(1), image::imageops::FilterType::Triangle)
        }
        _ => image,
    };
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    image
        .save(out)
        .map_err(|e| GameError::Capture(format!("写 PNG `{}` 失败: {e}", out.display())))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_matching_is_case_insensitive_substring() {
        assert!(title_matches("minecraft", "Minecraft 1.21.4 - 单人游戏"));
        assert!(title_matches("AliceInCradle", "AliceInCradle ver030"));
        assert!(title_matches("  steam  ", "Steam"));
        assert!(!title_matches("minecraft", "Terraria"));
        assert!(!title_matches("", "anything"));
    }

    #[test]
    fn window_info_serializes_flat() {
        let w = WindowInfo {
            title: "T".into(),
            app: "A".into(),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            minimized: false,
        };
        let j = serde_json::to_string(&w).unwrap();
        assert!(j.contains("\"width\":1920"), "{j}");
    }
}
