//! MCP (Model Context Protocol) connectors — lets the review LLM call
//! external tools (e.g. Context7 for current library docs) during review.
//!
//! Remote HTTP/SSE servers only, instance-wide config, per-repo opt-in via
//! `.codasaurus.toml`'s `checks.mcp_tools`. Structurally inert in offline
//! mode: [`list_enabled_tools`] always returns empty there.

pub mod cache;
pub mod catalog;
pub mod client;

use crate::db::DbPool;
use anyhow::{bail, Result};
use serde_json::Value;

/// Heuristic classification of a tool as state-mutating from its name. There is no
/// standard MCP field for this, and remote servers are third-party (GitHub, Linear,
/// Notion, Jira, Sentry, or an operator-supplied custom URL) — so tool calls are
/// steerable by whatever a reviewing PR's diff/description says, including a
/// jailbroken or hallucinating model. Default-deny anything that looks like a write
/// so a prompt-injected diff can only ever reach read-only tools; this is a
/// heuristic safety net, not a substitute for per-tool allowlisting by the operator.
fn is_write_tool(name: &str) -> bool {
    const WRITE_PREFIXES: &[&str] = &[
        "create",
        "update",
        "delete",
        "remove",
        "write",
        "put",
        "post",
        "patch",
        "set",
        "archive",
        "close",
        "merge",
        "publish",
        "send",
        "edit",
        "add",
        "invite",
        "revoke",
        "ban",
        "execute",
        "run",
        "trigger",
        "deploy",
        "assign",
        "move",
        "rename",
        "restore",
        "approve",
        "reject",
        "comment",
        "reply",
        "upload",
        "commit",
        "push",
        "fork",
        "star",
        "watch",
        "subscribe",
        "unsubscribe",
        "cancel",
        "resolve",
        "reopen",
        "lock",
        "unlock",
        "pin",
        "unpin",
    ];
    let lower = name.to_ascii_lowercase();
    WRITE_PREFIXES
        .iter()
        .any(|p| lower.starts_with(p) || lower.contains(&format!("_{p}")))
}

/// One tool exposed to the LLM, qualified with its server so names never collide.
#[derive(Debug, Clone)]
pub struct McpToolSpec {
    /// `"<server_id>__<tool_name>"`
    pub qualified_name: String,
    pub description: String,
    pub input_schema: Value,
}

/// Instance-wide server record merged from the static catalog + DB overrides.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ServerInfo {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub docs_url: Option<String>,
    pub enabled: bool,
    pub key_configured: bool,
    pub is_custom: bool,
}

async fn custom_server_ids(pool: &DbPool) -> Vec<String> {
    match crate::db::config::get_config(pool, "mcp_custom_server_ids").await {
        Ok(Some(raw)) => serde_json::from_str(&raw).unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// All servers known to this instance: catalog entries plus any custom ones,
/// merged with their DB-stored enabled/key state. Never returns raw keys.
pub async fn list_servers(pool: &DbPool) -> Vec<ServerInfo> {
    let mut out = Vec::new();

    for entry in catalog::CATALOG {
        let enabled = crate::db::config::get_config(pool, &format!("mcp_{}_enabled", entry.id))
            .await
            .ok()
            .flatten()
            .map(|v| v == "true")
            .unwrap_or(false);
        let key_configured =
            crate::db::config::get_config(pool, &format!("mcp_{}_api_key", entry.id))
                .await
                .ok()
                .flatten()
                .map(|v| !v.is_empty())
                .unwrap_or(false);
        out.push(ServerInfo {
            id: entry.id.to_string(),
            name: entry.name.to_string(),
            base_url: entry.base_url.to_string(),
            docs_url: Some(entry.docs_url.to_string()),
            enabled,
            key_configured,
            is_custom: false,
        });
    }

    for id in custom_server_ids(pool).await {
        let enabled = crate::db::config::get_config(pool, &format!("mcp_{id}_enabled"))
            .await
            .ok()
            .flatten()
            .map(|v| v == "true")
            .unwrap_or(false);
        let key_configured = crate::db::config::get_config(pool, &format!("mcp_{id}_api_key"))
            .await
            .ok()
            .flatten()
            .map(|v| !v.is_empty())
            .unwrap_or(false);
        let name = crate::db::config::get_config(pool, &format!("mcp_{id}_name"))
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| id.clone());
        let base_url = crate::db::config::get_config(pool, &format!("mcp_{id}_base_url"))
            .await
            .ok()
            .flatten()
            .unwrap_or_default();
        out.push(ServerInfo {
            id,
            name,
            base_url,
            docs_url: None,
            enabled,
            key_configured,
            is_custom: true,
        });
    }

    out
}

/// Tool specs for every enabled server, in OpenAI-compatible shape. Empty in
/// offline mode or when a server's `tools/list` call fails (fails open —
/// one broken server doesn't kill review).
pub async fn list_enabled_tools(pool: &DbPool) -> Vec<McpToolSpec> {
    if crate::registry::offline_mode() {
        return Vec::new();
    }

    let mut specs = Vec::new();
    for server in list_servers(pool).await {
        if !server.enabled || server.base_url.is_empty() {
            continue;
        }
        let api_key = crate::db::config::get_config(pool, &format!("mcp_{}_api_key", server.id))
            .await
            .ok()
            .flatten()
            .unwrap_or_default();

        let tools = cache::get_or_fetch_tools(&server.id, &server.base_url, &api_key).await;
        for tool in tools {
            let Some(name) = tool.get("name").and_then(|v| v.as_str()) else {
                continue;
            };
            if is_write_tool(name) {
                continue;
            }
            specs.push(McpToolSpec {
                qualified_name: format!("{}__{name}", server.id),
                description: tool
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                input_schema: tool
                    .get("inputSchema")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({"type": "object"})),
            });
        }
    }
    specs
}

/// Truncated at 8000 chars — bounds what a tool result can cost in the next
/// LLM round-trip, matching `llm::truncate_chars`'s cap on diffs.
const MAX_TOOL_RESULT_CHARS: usize = 8000;

pub async fn call_tool(pool: &DbPool, qualified_name: &str, arguments: &Value) -> Result<String> {
    let Some((server_id, tool_name)) = qualified_name.split_once("__") else {
        bail!("malformed MCP tool name: {qualified_name}");
    };

    // Re-validate against the currently enabled server + its live tool list before
    // executing anything — the model only sees this list when tools are *offered*,
    // but a jailbroken/hallucinating model can still emit a call for a name it was
    // never given (e.g. lifted from injected PR content), or for a server that was
    // disabled after tools were listed for this review. Never trust the tool call
    // in isolation from the current allowlist.
    let server = list_servers(pool)
        .await
        .into_iter()
        .find(|s| s.id == server_id)
        .ok_or_else(|| anyhow::anyhow!("unknown MCP server: {server_id}"))?;
    if !server.enabled || server.base_url.is_empty() {
        bail!("MCP server {server_id} is not enabled");
    }
    let api_key = crate::db::config::get_config(pool, &format!("mcp_{server_id}_api_key"))
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    let available = cache::get_or_fetch_tools(&server.id, &server.base_url, &api_key).await;
    let allowed = available
        .iter()
        .any(|t| t.get("name").and_then(|v| v.as_str()) == Some(tool_name));
    if !allowed {
        bail!("tool {tool_name} is not in {server_id}'s current tool list");
    }
    if is_write_tool(tool_name) {
        bail!("tool {qualified_name} looks state-mutating; write tools are not permitted");
    }

    let result = client::tools_call(&server.base_url, &api_key, tool_name, arguments).await;

    crate::db::audit::log_event(pool, "mcp.tool_called", None, Some(qualified_name), None).await;

    let text = match &result {
        Ok(v) => v.to_string(),
        Err(e) => format!("Error calling {qualified_name}: {e}"),
    };
    let truncated: String = text.chars().take(MAX_TOOL_RESULT_CHARS).collect();
    Ok(truncated)
}
