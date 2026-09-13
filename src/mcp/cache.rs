//! TTL cache for `tools/list` results — mirrors `crate::registry`'s cache
//! pattern so reviews don't refetch tool schemas from every MCP server.

use serde_json::Value;
use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};
use std::time::{Duration, Instant};

const TTL_SECS: u64 = 300;

static CACHE: LazyLock<RwLock<HashMap<String, (Vec<Value>, Instant)>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

pub async fn get_or_fetch_tools(server_id: &str, base_url: &str, api_key: &str) -> Vec<Value> {
    {
        let cache = CACHE.read().unwrap_or_else(|e| e.into_inner());
        if let Some((tools, time)) = cache.get(server_id) {
            if time.elapsed() < Duration::from_secs(TTL_SECS) {
                return tools.clone();
            }
        }
    }

    match super::client::tools_list(base_url, api_key).await {
        Ok(tools) => {
            let mut cache = CACHE.write().unwrap_or_else(|e| e.into_inner());
            cache.insert(server_id.to_string(), (tools.clone(), Instant::now()));
            tools
        }
        Err(e) => {
            tracing::warn!(server_id, error = %e, "MCP tools/list failed; skipping server");
            Vec::new()
        }
    }
}
