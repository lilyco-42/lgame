//! 轨迹 —— 每一步的「看了什么 / 做了什么 / 有什么警告」。
//!
//! 两个用途：
//! 1. 喂回给策略（最近几步进提示词，模型才知道自己刚干了什么）
//! 2. 落盘成 JSONL，事后可复盘、可重放、可当训练数据

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::action::Action;
use crate::error::Result;

/// 一步的完整记录
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepRecord {
    /// 对应第几次观察
    pub tick: u64,
    pub action: Action,
    /// 本步看到的画面
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame: Option<PathBuf>,
    /// 执行时的警告（例如「躯体不支持该动词」）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

impl StepRecord {
    pub fn new(tick: u64, action: Action) -> Self {
        StepRecord {
            tick,
            action,
            frame: None,
            warnings: Vec::new(),
        }
    }

    pub fn with_frame(mut self, frame: Option<PathBuf>) -> Self {
        self.frame = frame;
        self
    }

    pub fn with_warnings(mut self, warnings: Vec<String>) -> Self {
        self.warnings = warnings;
        self
    }

    /// 一行摘要
    pub fn summary(&self) -> String {
        let mut s = format!("t{} {}", self.tick, self.action.summary());
        if !self.warnings.is_empty() {
            s.push_str(&format!("（{} 条警告）", self.warnings.len()));
        }
        s
    }
}

/// 追加式 JSONL 轨迹文件
pub struct Trace {
    path: PathBuf,
    file: Option<std::fs::File>,
}

impl Trace {
    /// 在 `dir` 下新建 `trace.jsonl`（若 `dir` 不存在则创建）
    pub fn create(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join("trace.jsonl");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        Ok(Trace {
            path,
            file: Some(file),
        })
    }

    /// 只记录路径、不落盘（`--dry-run` 且不想留痕时用）
    pub fn inert(dir: &Path) -> Self {
        Trace {
            path: dir.join("trace.jsonl"),
            file: None,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn is_enabled(&self) -> bool {
        self.file.is_some()
    }

    /// 追加一步（每行一个 JSON 对象）
    pub fn append(&mut self, rec: &StepRecord) -> Result<()> {
        let Some(f) = self.file.as_mut() else {
            return Ok(());
        };
        let line = serde_json::to_string(rec)?;
        writeln!(f, "{line}")?;
        f.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_record_roundtrips_and_summarizes() {
        let r = StepRecord::new(
            3,
            Action {
                forward: 1.0,
                duration_ms: 300,
                ..Default::default()
            },
        )
        .with_warnings(vec!["w".into()]);
        let j = serde_json::to_string(&r).unwrap();
        let back: StepRecord = serde_json::from_str(&j).unwrap();
        assert_eq!(back.tick, 3);
        assert_eq!(back.warnings.len(), 1);
        assert!(r.summary().contains("1 条警告"), "{}", r.summary());
    }

    #[test]
    fn empty_warnings_are_omitted_from_json() {
        let r = StepRecord::new(1, Action::default());
        let j = serde_json::to_string(&r).unwrap();
        assert!(!j.contains("warnings"), "{j}");
        assert!(!j.contains("frame"), "{j}");
    }

    #[test]
    fn trace_appends_one_json_object_per_line() {
        let dir = std::env::temp_dir().join("lgame-test-trace");
        let _ = std::fs::remove_dir_all(&dir);

        let mut t = Trace::create(&dir).unwrap();
        assert!(t.is_enabled());
        t.append(&StepRecord::new(1, Action::wait(100))).unwrap();
        t.append(&StepRecord::new(2, Action::wait(200))).unwrap();
        drop(t);

        let text = std::fs::read_to_string(dir.join("trace.jsonl")).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text}");
        let first: StepRecord = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first.tick, 1);
        assert_eq!(first.action.duration_ms, 100);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn inert_trace_writes_nothing() {
        let dir = std::env::temp_dir().join("lgame-test-inert");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut t = Trace::inert(&dir);
        assert!(!t.is_enabled());
        t.append(&StepRecord::new(1, Action::wait(1))).unwrap();
        assert!(!dir.join("trace.jsonl").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
