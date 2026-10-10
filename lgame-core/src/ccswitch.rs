//! 「自己的免费 api」在哪里 —— 端点发现。
//!
//! # 为什么不去解析 cc-switch 的 SQLite
//!
//! cc-switch 是 Tauri 应用，凭据存在 `~/.cc-switch/cc-switch.db`（SQLite）。
//! 但**它接管 Claude 配置时会把最终端点写进 `~/.claude/settings.json`**：
//!
//! ```json
//! { "env": {
//!     "ANTHROPIC_BASE_URL": "http://127.0.0.1:15721",
//!     "ANTHROPIC_AUTH_TOKEN": "PROXY_MANAGED",
//!     "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-sonnet-5[1M]"
//! } }
//! ```
//!
//! 所以读一个 JSON 就够了 —— 不用引 SQLite，也不用碰任何密钥文件。
//! `PROXY_MANAGED` 表示真实凭据由代理注入，客户端送什么 token 都行。
//!
//! 优先级：环境变量 > `~/.claude/settings.json`。

use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{GameError, Result};
use crate::util::{home_dir, split_url};

/// 一个 Anthropic 兼容端点
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Endpoint {
    /// 例如 `http://127.0.0.1:15721`
    pub base_url: String,
    /// 令牌；`PROXY_MANAGED` 表示由本地代理注入
    pub api_key: String,
    pub model: String,
    /// 端点来源：`env` / `claude-settings`
    pub source: String,
    /// 探测时端口是否在听（本地代理没启动时为 false）
    pub reachable: bool,
    /// 给人和 Agent 看的补充说明；缺省不写进 JSON
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Endpoint {
    /// 是否走本地 cc-switch 代理
    pub fn is_local_proxy(&self) -> bool {
        matches!(split_url(&self.base_url), Some((h, _, _)) if h == "127.0.0.1" || h == "localhost")
    }

    /// `/v1/messages` 完整地址
    pub fn messages_url(&self) -> String {
        messages_url(&self.base_url)
    }
}

/// 把 base_url 规范成 messages 端点（容忍带不带 `/v1` 后缀）
pub fn messages_url(base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    if base.ends_with("/v1/messages") {
        base.to_string()
    } else if base.ends_with("/v1") {
        format!("{base}/messages")
    } else {
        format!("{base}/v1/messages")
    }
}

/// TCP 探活：本地代理没启动时 15721 不会在听
pub fn probe(base_url: &str) -> bool {
    let Some((host, port, _)) = split_url(base_url) else {
        return false;
    };
    let Ok(addrs) = (host.as_str(), port).to_socket_addrs() else {
        return false;
    };
    addrs
        .into_iter()
        .any(|addr| TcpStream::connect_timeout(&addr, Duration::from_millis(400)).is_ok())
}

/// 从 `~/.claude/settings.json` 读出的端点片段
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClaudeEnv {
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub model: Option<String>,
}

fn non_empty(v: Option<&serde_json::Value>) -> Option<String> {
    v.and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 读取 `~/.claude/settings.json` 的 `env` 段（cc-switch 接管后写在这里）
pub fn read_claude_settings() -> Option<ClaudeEnv> {
    let path = home_dir()?.join(".claude").join("settings.json");
    let text = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let env = v.get("env")?;
    Some(ClaudeEnv {
        base_url: non_empty(env.get("ANTHROPIC_BASE_URL")),
        api_key: non_empty(env.get("ANTHROPIC_AUTH_TOKEN"))
            .or_else(|| non_empty(env.get("ANTHROPIC_API_KEY"))),
        // 依次退到 sonnet / opus / haiku / 通用 model —— 只要有一个能填就行
        model: non_empty(env.get("ANTHROPIC_DEFAULT_SONNET_MODEL"))
            .or_else(|| non_empty(env.get("ANTHROPIC_DEFAULT_OPUS_MODEL")))
            .or_else(|| non_empty(env.get("ANTHROPIC_MODEL")))
            .or_else(|| non_empty(env.get("ANTHROPIC_DEFAULT_HAIKU_MODEL"))),
    })
}

fn env_var(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 纯函数版合并逻辑（便于单测，不碰环境变量）
pub fn resolve_from(
    env_base: Option<String>,
    env_key: Option<String>,
    env_model: Option<String>,
    settings: Option<ClaudeEnv>,
) -> Result<Endpoint> {
    let s = settings.unwrap_or_default();

    let (base_url, source) = match (env_base, s.base_url.clone()) {
        (Some(b), _) => (b, "env".to_string()),
        (None, Some(b)) => (b, "claude-settings".to_string()),
        (None, None) => {
            return Err(GameError::Config(
                "找不到本地 LLM 端点。三条路任选其一：\n\
                 1) 启动 cc-switch —— 它会把代理地址写进 ~/.claude/settings.json 的 \
                    env.ANTHROPIC_BASE_URL（默认 http://127.0.0.1:15721）\n\
                 2) 显式给：LGAME_LLM_BASE_URL / LGAME_LLM_API_KEY / LGAME_LLM_MODEL\n\
                 3) 不调 LLM：用 `--replay` 走离线脚本策略"
                    .to_string(),
            ));
        }
    };

    let api_key = env_key
        .or(s.api_key)
        .unwrap_or_else(|| "PROXY_MANAGED".to_string());
    let model = env_model
        .or(s.model)
        .unwrap_or_else(|| "claude-sonnet-5".to_string());

    let reachable = probe(&base_url);
    let note = if !reachable {
        Some(format!(
            "端点 {base_url} 端口没有在听 —— 如果是本地 cc-switch 代理，请先启动 cc-switch"
        ))
    } else if api_key == "PROXY_MANAGED" {
        Some("令牌为 PROXY_MANAGED：真实凭据由本地代理注入".to_string())
    } else {
        None
    };

    Ok(Endpoint {
        base_url,
        api_key,
        model,
        source,
        reachable,
        note,
    })
}

/// 发现端点（读环境变量 + `~/.claude/settings.json`）
pub fn discover() -> Result<Endpoint> {
    resolve_from(
        env_var("LGAME_LLM_BASE_URL"),
        env_var("LGAME_LLM_API_KEY"),
        env_var("LGAME_LLM_MODEL"),
        read_claude_settings(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(base: &str, key: &str, model: &str) -> ClaudeEnv {
        ClaudeEnv {
            base_url: Some(base.into()),
            api_key: Some(key.into()),
            model: Some(model.into()),
        }
    }

    #[test]
    fn messages_url_normalizes_suffixes() {
        assert_eq!(
            messages_url("http://127.0.0.1:15721"),
            "http://127.0.0.1:15721/v1/messages"
        );
        assert_eq!(
            messages_url("http://127.0.0.1:15721/"),
            "http://127.0.0.1:15721/v1/messages"
        );
        assert_eq!(
            messages_url("https://api.deepseek.com/anthropic"),
            "https://api.deepseek.com/anthropic/v1/messages"
        );
        assert_eq!(
            messages_url("https://x/anthropic/v1"),
            "https://x/anthropic/v1/messages"
        );
        assert_eq!(
            messages_url("https://x/v1/messages"),
            "https://x/v1/messages"
        );
    }

    #[test]
    fn env_wins_over_settings() {
        let ep = resolve_from(
            Some("http://127.0.0.1:9999".into()),
            None,
            None,
            Some(settings("http://127.0.0.1:15721", "PROXY_MANAGED", "m1")),
        )
        .unwrap();
        assert_eq!(ep.base_url, "http://127.0.0.1:9999");
        assert_eq!(ep.source, "env");
        // 未覆盖的字段仍然从 settings 兜底
        assert_eq!(ep.api_key, "PROXY_MANAGED");
        assert_eq!(ep.model, "m1");
    }

    #[test]
    fn falls_back_to_settings_when_env_absent() {
        let ep = resolve_from(
            None,
            None,
            None,
            Some(settings(
                "http://127.0.0.1:15721",
                "PROXY_MANAGED",
                "claude-sonnet-5[1M]",
            )),
        )
        .unwrap();
        assert_eq!(ep.source, "claude-settings");
        assert_eq!(ep.model, "claude-sonnet-5[1M]");
        assert!(ep.is_local_proxy());
    }

    #[test]
    fn missing_everything_gives_actionable_error() {
        let err = resolve_from(None, None, None, None).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("cc-switch"), "{msg}");
        assert!(msg.contains("LGAME_LLM_BASE_URL"), "{msg}");
        assert!(msg.contains("--replay"), "{msg}");
    }

    #[test]
    fn api_key_defaults_to_proxy_managed() {
        let ep = resolve_from(
            None,
            None,
            None,
            Some(ClaudeEnv {
                base_url: Some("http://127.0.0.1:15721".into()),
                api_key: None,
                model: None,
            }),
        )
        .unwrap();
        assert_eq!(ep.api_key, "PROXY_MANAGED");
    }

    #[test]
    fn unreachable_endpoint_is_flagged_not_fatal() {
        // 端口 1 上不会有服务
        let ep = resolve_from(Some("http://127.0.0.1:1".into()), None, None, None).unwrap();
        assert!(!ep.reachable);
        assert!(ep.note.as_deref().unwrap_or("").contains("没有在听"));
    }

    #[test]
    fn is_local_proxy_recognizes_loopback() {
        assert!(Endpoint {
            base_url: "http://localhost:15721".into(),
            api_key: "x".into(),
            model: "m".into(),
            source: "env".into(),
            reachable: true,
            note: None,
        }
        .is_local_proxy());
        assert!(!Endpoint {
            base_url: "https://api.deepseek.com/anthropic".into(),
            api_key: "x".into(),
            model: "m".into(),
            source: "env".into(),
            reachable: true,
            note: None,
        }
        .is_local_proxy());
    }
}
