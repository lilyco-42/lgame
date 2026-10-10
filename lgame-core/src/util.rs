//! 零依赖小工具：base64 编码 + 家目录。
//!
//! 刻意不引 `base64` crate —— 一个 30 行的编码器配一组单测，
//! 比多一个依赖更省事（也避免依赖版本漂移）。

use std::path::PathBuf;

const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// 标准 base64 编码（带 `=` 填充）
pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;

        out.push(TABLE[((n >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((n >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(n & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// 用户家目录（Windows 用 `USERPROFILE`，类 Unix 用 `HOME`）
pub fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

/// 把可能带引号 / 空白的字符串清干净（LLM 常吐 ```json 围栏）
pub fn strip_code_fence(s: &str) -> &str {
    let t = s.trim();
    let t = t.strip_prefix("```json").unwrap_or(t);
    let t = t.strip_prefix("```JSON").unwrap_or(t);
    let t = t.strip_prefix("```").unwrap_or(t);
    let t = t.strip_suffix("```").unwrap_or(t);
    t.trim()
}

/// 从一段文本里抠出第一个平衡的 JSON 对象
///
/// LLM 经常在 JSON 前后加解释（"好的，我决定：{...} 这样应该能前进"），
/// 所以不能直接 `serde_json::from_str` 整段。
pub fn extract_json_object(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    let start = s.find('{')?;
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escaped = false;

    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_str {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return s.get(start..=i);
                }
            }
            _ => {}
        }
    }
    None
}

/// 窗口标题是否匹配 `pattern`（大小写不敏感的子串匹配）
///
/// 刻意用子串而不是精确相等：游戏标题常带版本号 / 状态后缀
/// （`Minecraft 1.21.4 - 单人游戏`、`AliceInCradle ver030`），
/// 精确相等会让用户每次升级都要改配置。
///
/// 放在 `util` 而不是 `capture`，是因为关掉 `screen` 特性后
/// [`crate::targets`] 仍然需要它。
pub fn title_matches(pattern: &str, title: &str) -> bool {
    let p = pattern.trim();
    if p.is_empty() {
        return false;
    }
    title.to_ascii_lowercase().contains(&p.to_ascii_lowercase())
}

/// 最小 URL 拆解：`http://host:port/path` → `(host, port, path)`
///
/// 只为「探测本地代理是否在听」和「拼 `/v1/messages`」服务，
/// 不做完整 URL 解析（不处理 userinfo / IPv6 字面量）。
pub fn split_url(url: &str) -> Option<(String, u16, String)> {
    let (scheme, rest) = match url.split_once("://") {
        Some((s, r)) => (s.to_ascii_lowercase(), r),
        None => ("http".to_string(), url),
    };
    let default_port = if scheme == "https" { 443 } else { 80 };
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse::<u16>().ok()?),
        None => (authority.to_string(), default_port),
    };
    if host.is_empty() {
        return None;
    }
    Some((host, port, path.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_known_vectors() {
        // RFC 4648 测试向量
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn base64_handles_binary_roundtrip_length() {
        let data: Vec<u8> = (0u8..=255).collect();
        let enc = base64_encode(&data);
        assert_eq!(enc.len(), 342 + 2); // 256 bytes → 344 chars with padding
        assert!(enc.ends_with("==") || enc.ends_with('='));
    }

    #[test]
    fn strip_code_fence_removes_wrappers() {
        assert_eq!(strip_code_fence("```json\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(strip_code_fence("```\n{}\n```"), "{}");
        assert_eq!(strip_code_fence("  {\"a\":1}  "), "{\"a\":1}");
    }

    #[test]
    fn extract_json_object_ignores_surrounding_prose() {
        let s = "好的，我决定：{\"forward\": 1.0, \"duration_ms\": 500} 这样应该能前进";
        assert_eq!(
            extract_json_object(s),
            Some("{\"forward\": 1.0, \"duration_ms\": 500}")
        );
    }

    #[test]
    fn extract_json_object_handles_nested_and_braces_in_strings() {
        let s = r#"prefix {"a":{"b":1},"c":"} not the end {"} suffix"#;
        assert_eq!(
            extract_json_object(s),
            Some(r#"{"a":{"b":1},"c":"} not the end {"}"#)
        );
    }

    #[test]
    fn extract_json_object_returns_none_without_json() {
        assert_eq!(extract_json_object("no json here"), None);
        assert_eq!(extract_json_object("{ unbalanced"), None);
    }

    #[test]
    fn split_url_parses_host_port_path() {
        assert_eq!(
            split_url("http://127.0.0.1:15721"),
            Some(("127.0.0.1".to_string(), 15721, "".to_string()))
        );
        assert_eq!(
            split_url("https://api.deepseek.com/anthropic"),
            Some((
                "api.deepseek.com".to_string(),
                443,
                "/anthropic".to_string()
            ))
        );
        assert_eq!(
            split_url("http://localhost"),
            Some(("localhost".to_string(), 80, "".to_string()))
        );
    }
}
