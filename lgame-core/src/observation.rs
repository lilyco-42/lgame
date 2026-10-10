//! 观察 —— 中间层的「输入」。策略看的就是这个。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 一次观察结果
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Observation {
    /// 第几次观察（从 1 开始）
    pub tick: u64,
    /// 目标标识（窗口标题 / 机器人名）
    pub target: String,
    /// 画面文件路径（PNG）。纯结构化躯体（无摄像头）可为 `None`
    pub frame: Option<PathBuf>,
    /// 结构化状态 —— 原生桥专用。
    ///
    /// 游戏里由引擎调试协议填（坐标、血量、背包）；机器人上由关节编码器填。
    /// 屏幕档位下为 `None`（只能靠画面）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<serde_json::Value>,
    /// 补充说明（例如「窗口最小化，画面可能不刷新」）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Observation {
    pub fn new(tick: u64, target: impl Into<String>) -> Self {
        Observation {
            tick,
            target: target.into(),
            frame: None,
            state: None,
            note: None,
        }
    }

    pub fn with_frame(mut self, path: impl Into<PathBuf>) -> Self {
        self.frame = Some(path.into());
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    /// 一行摘要（trace 用）
    pub fn summary(&self) -> String {
        let frame = self
            .frame
            .as_ref()
            .map(|p| {
                p.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| p.display().to_string())
            })
            .unwrap_or_else(|| "(无画面)".to_string());
        match &self.note {
            Some(n) => format!("tick {} · {} · {}", self.tick, frame, n),
            None => format!("tick {} · {}", self.tick, frame),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_sets_frame_and_note() {
        let o = Observation::new(3, "Minecraft")
            .with_frame("a/tick-00003.png")
            .with_note("窗口最小化");
        assert_eq!(o.tick, 3);
        assert_eq!(o.target, "Minecraft");
        assert!(o.summary().contains("tick-00003.png"));
        assert!(o.summary().contains("窗口最小化"));
    }

    #[test]
    fn summary_handles_missing_frame() {
        let o = Observation::new(1, "robot");
        assert!(o.summary().contains("(无画面)"));
    }

    #[test]
    fn optional_fields_are_omitted_from_json() {
        let o = Observation::new(1, "x");
        let j = serde_json::to_string(&o).unwrap();
        assert!(!j.contains("state"), "{j}");
        assert!(!j.contains("note"), "{j}");
    }
}
