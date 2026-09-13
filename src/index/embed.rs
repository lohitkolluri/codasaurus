//! `/embeddings` calls for the semantic index (pgvector layer on `repo_symbols`).
//! Reuses the same OpenAI-compatible HTTP conventions as `crate::llm` (client,
//! SSRF check, offline-mode gate) rather than a second HTTP stack.

use anyhow::Result;
use serde_json::json;

/// Embed a batch of texts (≤100, per OpenAI's `/embeddings` batch limit).
/// Returns `Ok(vec![])` when offline — callers should treat that as "skip
/// indexing this batch", not an error.
pub async fn embed_texts(
    config: &crate::llm::LlmConfig,
    texts: &[String],
) -> Result<Vec<Vec<f32>>> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    if crate::bot::offline::offline_mode_from_env_and_db(None) {
        return Ok(Vec::new());
    }
    crate::llm::assert_embedding_endpoint_safe(config).await?;

    let client = crate::llm::shared_client()?;
    let url = format!("{}/embeddings", config.base_url.trim_end_matches('/'));
    let body = json!({
        "model": config.embedding_model,
        "input": texts,
    });
    let resp = client
        .post(&url)
        .bearer_auth(&config.api_key)
        .json(&body)
        .send()
        .await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!(
            "embeddings endpoint returned {status}: {}",
            text.chars().take(300).collect::<String>()
        );
    }
    let parsed: serde_json::Value = resp.json().await?;
    let data = parsed["data"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("embeddings response missing `data` array"))?;
    let mut out: Vec<Vec<f32>> = data
        .iter()
        .map(|d| {
            d["embedding"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_f64())
                        .map(|v| v as f32)
                        .collect()
                })
                .unwrap_or_default()
        })
        .collect();
    out.retain(|v| !v.is_empty());
    Ok(out)
}
