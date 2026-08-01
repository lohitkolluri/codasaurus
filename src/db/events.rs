//! Lightweight agent event spine (audit + cost ledger).

use crate::db::{db_execute, db_scalar, DbPool};

/// Append an LLM call event (best-effort; never fails the review).
#[allow(clippy::too_many_arguments)]
pub async fn emit_llm_call(
    pool: &DbPool,
    agent: &str,
    model: &str,
    prompt_chars: usize,
    max_out_tokens: u32,
    strong: bool,
    latency_ms: u64,
    outcome: &str,
) {
    let tokens_in = (prompt_chars / 4) as i64;
    let tokens_out = ((max_out_tokens as f64) * 0.4) as i64;
    let micros = crate::llm::estimate_spend_microdollars(prompt_chars, max_out_tokens, strong);
    let cost = micros as f64 / 1_000_000.0;
    if let Err(e) = db_execute!(
        pool,
        "INSERT INTO agent_events
           (agent, event_type, model, tokens_in, tokens_out, cost_usd_est, latency_ms, outcome)
         VALUES (?, 'llm.call', ?, ?, ?, ?, ?, ?)",
        agent,
        model,
        tokens_in,
        tokens_out,
        cost,
        latency_ms as i64,
        outcome
    ) {
        tracing::debug!(error = %e, "agent_events insert skipped");
    }
}

/// Sum estimated spend for the last ~24 hours (process observability + BudgetGuard).
pub async fn spend_usd_last_day(pool: &DbPool) -> f64 {
    db_scalar!(
        pool,
        f64,
        "SELECT COALESCE(SUM(cost_usd_est), 0)
         FROM agent_events
         WHERE event_type = 'llm.call'
           AND ts >= NOW() - INTERVAL '1 day'"
    )
    .unwrap_or(0.0)
}

/// Drop old event rows (retention).
pub async fn prune_older_than_days(pool: &DbPool, days: i64) {
    if let Err(e) =
        sqlx::query("DELETE FROM agent_events WHERE ts < NOW() - ($1::bigint * INTERVAL '1 day')")
            .bind(days)
            .execute(pool.as_pg())
            .await
    {
        tracing::debug!(error = %e, "agent_events prune skipped");
    }
}
