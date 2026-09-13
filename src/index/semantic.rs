//! pgvector layer on top of the `repo_symbols` graph (migration v22): finds
//! symbols that are semantically related to a diff's changed symbols even
//! when they aren't directly imported/called (which `store::callers_of`
//! alone can't find).

use crate::db::DbPool;
use crate::index::extract::FileIndex;
use sha2::{Digest, Sha256};
use sqlx::Row;

pub struct RelatedSymbol {
    pub file_path: String,
    pub symbol_name: String,
    pub line: i64,
    pub signature: String,
    pub distance: f64,
}

fn embed_text_for(symbol_name: &str, signature: Option<&str>) -> String {
    match signature {
        Some(sig) if !sig.trim().is_empty() => format!("{symbol_name}\n{sig}"),
        _ => symbol_name.to_string(),
    }
}

fn content_hash(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    hex::encode(hasher.finalize())
}

fn vector_literal(v: &[f32]) -> String {
    let mut s = String::from("[");
    for (i, x) in v.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&x.to_string());
    }
    s.push(']');
    s
}

/// Re-embed changed/new symbols for one repo, skipping unchanged ones via
/// `content_hash`. Called from the same job that runs `store::replace_*_index`
/// so embeddings never drift out of lockstep with the symbol graph.
pub async fn reindex_repo_embeddings(
    pool: &DbPool,
    llm_cfg: &crate::llm::LlmConfig,
    repo_full_name: &str,
    files: &[FileIndex],
) -> anyhow::Result<()> {
    struct Pending<'a> {
        file_path: &'a str,
        symbol_name: &'a str,
        line: i64,
        text: String,
        hash: String,
    }

    let mut pending: Vec<Pending> = Vec::new();
    for file in files {
        for sym in &file.symbols {
            let text = embed_text_for(&sym.name, sym.signature.as_deref());
            let hash = content_hash(&text);
            let existing: Option<String> = sqlx::query_scalar(
                "SELECT content_hash FROM repo_symbol_embeddings
                 WHERE repo_full_name = $1 AND file_path = $2 AND symbol_name = $3 AND line = $4",
            )
            .bind(repo_full_name)
            .bind(&file.file_path)
            .bind(&sym.name)
            .bind(sym.line)
            .fetch_optional(pool.as_pg())
            .await
            .unwrap_or(None);
            if existing.as_deref() == Some(hash.as_str()) {
                continue;
            }
            pending.push(Pending {
                file_path: &file.file_path,
                symbol_name: &sym.name,
                line: sym.line,
                text,
                hash,
            });
        }
    }
    if pending.is_empty() {
        return Ok(());
    }

    for batch in pending.chunks(100) {
        let texts: Vec<String> = batch.iter().map(|p| p.text.clone()).collect();
        let embeddings = crate::index::embed::embed_texts(llm_cfg, &texts).await?;
        if embeddings.len() != batch.len() {
            // Endpoint doesn't support embeddings, or returned a partial batch — fail
            // open for this batch rather than the whole review.
            continue;
        }
        for (p, emb) in batch.iter().zip(embeddings.iter()) {
            let _ = sqlx::query(
                "INSERT INTO repo_symbol_embeddings
                    (repo_full_name, file_path, symbol_name, line, content_hash, embedding, model)
                 VALUES ($1, $2, $3, $4, $5, $6::vector, $7)
                 ON CONFLICT (repo_full_name, file_path, symbol_name, line)
                 DO UPDATE SET content_hash = EXCLUDED.content_hash,
                               embedding = EXCLUDED.embedding,
                               model = EXCLUDED.model,
                               created_at = now()",
            )
            .bind(repo_full_name)
            .bind(p.file_path)
            .bind(p.symbol_name)
            .bind(p.line)
            .bind(&p.hash)
            .bind(vector_literal(emb))
            .bind(&llm_cfg.embedding_model)
            .execute(pool.as_pg())
            .await;
        }
    }
    Ok(())
}

/// Nearest neighbors (cosine distance) to `changed_symbols`, excluding the
/// files the diff already touches. Caps `k` and drops weak matches so the
/// grounding prompt doesn't fill up with noise.
pub async fn related_symbols(
    pool: &DbPool,
    llm_cfg: &crate::llm::LlmConfig,
    repo_full_name: &str,
    changed_symbols: &[String],
    changed_files: &[String],
    k: usize,
) -> anyhow::Result<Vec<RelatedSymbol>> {
    if changed_symbols.is_empty() {
        return Ok(Vec::new());
    }
    // embed_texts forwards the whole slice in one request; cap at the provider's
    // documented batch limit (see index::embed) instead of erroring out on large PRs.
    let symbols_capped = &changed_symbols[..changed_symbols.len().min(100)];
    let embeddings = crate::index::embed::embed_texts(llm_cfg, symbols_capped).await?;
    if embeddings.is_empty() {
        return Ok(Vec::new());
    }

    const DISTANCE_THRESHOLD: f64 = 0.5;
    let mut out: Vec<RelatedSymbol> = Vec::new();
    for emb in &embeddings {
        let lit = vector_literal(emb);
        let rows = sqlx::query(
            "SELECT file_path, symbol_name, line,
                    embedding <=> $1::vector AS distance
             FROM repo_symbol_embeddings
             WHERE repo_full_name = $2 AND NOT (file_path = ANY($3))
             ORDER BY distance ASC
             LIMIT $4",
        )
        .bind(&lit)
        .bind(repo_full_name)
        .bind(changed_files)
        .bind(k as i64)
        .fetch_all(pool.as_pg())
        .await
        .unwrap_or_default();
        for row in rows {
            let distance: f64 = row.get("distance");
            if distance > DISTANCE_THRESHOLD {
                continue;
            }
            out.push(RelatedSymbol {
                file_path: row.get("file_path"),
                symbol_name: row.get("symbol_name"),
                line: row.get("line"),
                signature: String::new(),
                distance,
            });
        }
    }
    out.sort_by(|a, b| a.distance.total_cmp(&b.distance));
    out.truncate(k);
    Ok(out)
}
