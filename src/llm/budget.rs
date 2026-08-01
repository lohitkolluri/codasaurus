//! Daily LLM spend hard-stop (BudgetGuard).

use std::sync::atomic::{AtomicU64, Ordering};

/// Process-local microdollars spent since boot (mirrors metrics; used when DB unavailable).
static LOCAL_SPEND_MICROS: AtomicU64 = AtomicU64::new(0);

pub fn record_local_spend_micros(micros: u64) {
    LOCAL_SPEND_MICROS.fetch_add(micros, Ordering::Relaxed);
}

/// Resolve daily budget in USD. `0` / unset = unlimited.
pub async fn daily_budget_usd(pool: Option<&crate::db::DbPool>) -> f64 {
    if let Ok(v) = std::env::var("CODASAURUS_LLM_DAILY_BUDGET_USD") {
        if let Ok(n) = v.parse::<f64>() {
            return n.max(0.0);
        }
    }
    if let Some(pool) = pool {
        if let Ok(Some(v)) = crate::db::config::get_config(pool, "llm_daily_budget_usd").await {
            if let Ok(n) = v.parse::<f64>() {
                return n.max(0.0);
            }
        }
    }
    0.0
}

/// Current spend estimate for the budget window (DB last-day preferred).
pub async fn current_spend_usd(pool: Option<&crate::db::DbPool>) -> f64 {
    if let Some(pool) = pool {
        let db = crate::db::events::spend_usd_last_day(pool).await;
        if db > 0.0 {
            return db;
        }
    }
    LOCAL_SPEND_MICROS.load(Ordering::Relaxed) as f64 / 1_000_000.0
}

/// Returns `Err` when the daily budget would be exceeded.
pub async fn assert_within_budget(pool: Option<&crate::db::DbPool>) -> anyhow::Result<()> {
    let budget = daily_budget_usd(pool).await;
    if budget <= 0.0 {
        return Ok(());
    }
    let spent = current_spend_usd(pool).await;
    if spent >= budget {
        anyhow::bail!(
            "LLM daily budget exceeded: spent ~${spent:.4} >= cap ${budget:.2} \
             (set llm_daily_budget_usd / CODASAURUS_LLM_DAILY_BUDGET_USD; 0 = unlimited)"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_spend_accumulates() {
        let before = LOCAL_SPEND_MICROS.load(Ordering::Relaxed);
        record_local_spend_micros(1_000);
        assert!(LOCAL_SPEND_MICROS.load(Ordering::Relaxed) >= before + 1_000);
    }
}
