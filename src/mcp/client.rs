//! Minimal JSON-RPC 2.0 client for remote MCP servers over Streamable HTTP.
//!
//! Deliberately narrow: only the three methods the review loop needs
//! (`initialize`, `tools/list`, `tools/call`). No stdio/subprocess transport —
//! that would mean spawning binaries influenced indirectly by PR content.

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::sync::LazyLock;
use std::time::Duration;

/// Bound memory from a misbehaving or malicious server.
const MAX_RESPONSE_BYTES: usize = 256 * 1024;

static CLIENT: LazyLock<Option<reqwest::Client>> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .ok()
});

async fn call_rpc(base_url: &str, api_key: &str, method: &str, params: Value) -> Result<Value> {
    crate::ssrf::validate_http_url_resolved(base_url, false)
        .await
        .map_err(anyhow::Error::msg)?;

    let client = CLIENT.as_ref().context("MCP HTTP client not available")?;
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params,
    });

    let mut req = client
        .post(base_url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .json(&body);
    if !api_key.is_empty() {
        req = req.bearer_auth(api_key);
    }

    let resp = req.send().await.context("MCP request failed")?;
    if !resp.status().is_success() {
        bail!("MCP server returned HTTP {}", resp.status());
    }
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let bytes = resp.bytes().await.context("reading MCP response body")?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        bail!("MCP response exceeded {MAX_RESPONSE_BYTES} bytes");
    }
    let text = String::from_utf8_lossy(&bytes);

    let raw_json = if content_type.contains("text/event-stream") {
        text.lines()
            .find_map(|l| l.strip_prefix("data:"))
            .map(str::trim)
            .context("no data event in MCP SSE response")?
            .to_string()
    } else {
        text.trim().to_string()
    };

    let rpc: Value = serde_json::from_str(&raw_json).context("invalid MCP JSON-RPC response")?;
    if let Some(err) = rpc.get("error") {
        bail!("MCP server error: {err}");
    }
    Ok(rpc.get("result").cloned().unwrap_or(Value::Null))
}

pub async fn initialize(base_url: &str, api_key: &str) -> Result<Value> {
    call_rpc(
        base_url,
        api_key,
        "initialize",
        json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "codasaurus", "version": env!("CARGO_PKG_VERSION") },
        }),
    )
    .await
}

pub async fn tools_list(base_url: &str, api_key: &str) -> Result<Vec<Value>> {
    let result = call_rpc(base_url, api_key, "tools/list", json!({})).await?;
    Ok(result
        .get("tools")
        .and_then(|t| t.as_array())
        .cloned()
        .unwrap_or_default())
}

pub async fn tools_call(
    base_url: &str,
    api_key: &str,
    tool_name: &str,
    arguments: &Value,
) -> Result<Value> {
    call_rpc(
        base_url,
        api_key,
        "tools/call",
        json!({ "name": tool_name, "arguments": arguments }),
    )
    .await
}
