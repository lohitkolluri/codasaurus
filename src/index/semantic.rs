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

    // One bulk read of the current hashes for these files, then O(1) lookups —
    // the per-symbol SELECT it replaces was one round-trip per symbol.
    let file_paths: Vec<String> = files.iter().map(|f| f.file_path.clone()).collect();
    let existing: std::collections::HashMap<(String, String, i64), String> = sqlx::query(
        "SELECT file_path, symbol_name, line, content_hash FROM repo_symbol_embeddings
         WHERE repo_full_name = $1 AND file_path = ANY($2)",
    )
    .bind(repo_full_name)
    .bind(&file_paths)
    .fetch_all(pool.as_pg())
    .await
    .unwrap_or_else(|e| {
        // Not fatal: an empty map just re-embeds everything, at a cost.
        tracing::warn!(error = %e, repo = repo_full_name, "embedding hash lookup failed; re-embedding all symbols");
        Vec::new()
    })
    .into_iter()
    .map(|r| ((r.get(0), r.get(1), r.get(2)), r.get(3)))
    .collect();

    let mut seen = std::collections::HashSet::new();
    let mut pending: Vec<Pending> = Vec::new();
    for file in files {
        for sym in &file.symbols {
            let key = (file.file_path.clone(), sym.name.clone(), sym.line);
            // A repeated key in one batch would break the INSERT's DO UPDATE.
            if !seen.insert(key.clone()) {
                continue;
            }
            let text = embed_text_for(&sym.name, sym.signature.as_deref());
            let hash = content_hash(&text);
            if existing.get(&key).map(String::as_str) == Some(hash.as_str()) {
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

    let batch_size = crate::config::load(None)
        .unwrap_or_default()
        .index
        .batch_size();
    for batch in pending.chunks(batch_size) {
        let texts: Vec<String> = batch.iter().map(|p| p.text.clone()).collect();
        let embeddings = crate::index::embed::embed_texts(llm_cfg, &texts).await?;
        if embeddings.len() != batch.len() {
            // Endpoint doesn't support embeddings, or returned a partial batch — fail
            // open for this batch rather than the whole review.
            continue;
        }
        let paths: Vec<String> = batch.iter().map(|p| p.file_path.to_string()).collect();
        let names: Vec<String> = batch.iter().map(|p| p.symbol_name.to_string()).collect();
        let lines: Vec<i64> = batch.iter().map(|p| p.line).collect();
        let hashes: Vec<String> = batch.iter().map(|p| p.hash.clone()).collect();
        let vectors: Vec<String> = embeddings.iter().map(|e| vector_literal(e)).collect();

        let written = sqlx::query(
            "INSERT INTO repo_symbol_embeddings
                (repo_full_name, file_path, symbol_name, line, content_hash, embedding, model)
             SELECT $1, f, s, l, h, v::vector, $7
             FROM UNNEST($2::text[], $3::text[], $4::bigint[], $5::text[], $6::text[])
                  AS t(f, s, l, h, v)
             ON CONFLICT (repo_full_name, file_path, symbol_name, line)
             DO UPDATE SET content_hash = EXCLUDED.content_hash,
                           embedding = EXCLUDED.embedding,
                           model = EXCLUDED.model,
                           created_at = now()",
        )
        .bind(repo_full_name)
        .bind(&paths)
        .bind(&names)
        .bind(&lines)
        .bind(&hashes)
        .bind(&vectors)
        .bind(&llm_cfg.embedding_model)
        .execute(pool.as_pg())
        .await;
        if let Err(e) = written {
            // Silently dropping this leaves the semantic index permanently empty
            // while every review pays to re-embed the same symbols.
            tracing::warn!(
                error = %e,
                repo = repo_full_name,
                batch = batch.len(),
                "persisting symbol embeddings failed; semantic grounding will be incomplete"
            );
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
    // Duplicate symbol names are common across a diff and cost tokens to re-embed.
    let index_cfg = crate::config::load(None).unwrap_or_default().index;
    let mut seen = std::collections::HashSet::new();
    let unique: Vec<String> = changed_symbols
        .iter()
        .filter(|s| seen.insert(s.as_str()))
        .take(index_cfg.max_query_symbols())
        .cloned()
        .collect();
    let embeddings = crate::index::embed::embed_texts(llm_cfg, &unique).await?;
    if embeddings.is_empty() {
        return Ok(Vec::new());
    }

    // One round-trip: a LATERAL ANN lookup per query vector (each still index-backed),
    // deduplicated to the best distance per symbol. Looping in Rust instead returned
    // the same symbol once per changed symbol, so the top-k could be k copies of one hit.
    let vectors: Vec<String> = embeddings.iter().map(|e| vector_literal(e)).collect();
    let rows = sqlx::query(
        "SELECT nn.file_path, nn.symbol_name, nn.line, MIN(nn.distance) AS distance
         FROM UNNEST($1::text[]) AS q(vec),
         LATERAL (
             SELECT file_path, symbol_name, line, embedding <=> q.vec::vector AS distance
             FROM repo_symbol_embeddings
             WHERE repo_full_name = $2 AND NOT (file_path = ANY($3))
             ORDER BY embedding <=> q.vec::vector ASC
             LIMIT $4
         ) nn
         GROUP BY nn.file_path, nn.symbol_name, nn.line
         HAVING MIN(nn.distance) <= $5
         ORDER BY distance ASC
         LIMIT $4",
    )
    .bind(&vectors)
    .bind(repo_full_name)
    .bind(changed_files)
    .bind(k as i64)
    .bind(index_cfg.distance_threshold())
    .fetch_all(pool.as_pg())
    .await
    .unwrap_or_else(|e| {
        tracing::warn!(error = %e, repo = repo_full_name, "related-symbol search failed; review loses semantic grounding");
        Vec::new()
    });

    Ok(rows
        .into_iter()
        .map(|row| RelatedSymbol {
            file_path: row.get("file_path"),
            symbol_name: row.get("symbol_name"),
            line: row.get("line"),
            signature: String::new(),
            distance: row.get("distance"),
        })
        .collect())
}
