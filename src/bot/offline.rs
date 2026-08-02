//! Offline / air-gap egress profile.

use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EgressProfile {
    /// Full network: registries, OSV, BYOK LLM allowed.
    Full,
    /// LLM may run to configured base URL; no assumption of public cloud.
    ByokOnly,
    /// No LLM; registry/OSV may still be blocked depending on flags.
    Offline,
}

impl EgressProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::ByokOnly => "byok-only",
            Self::Offline => "offline",
        }
    }
}

/// Resolve egress profile from env + DB-ish flags.
///
/// - `Offline` — fail-closed; no LLM / registry / OSV network
/// - `ByokOnly` — LLM configured (BYOK base URL or API key); registries allowed
/// - `Full` — not offline, no LLM configured (Tier-1 network only)
pub fn resolve_egress_profile(
    offline_mode: bool,
    llm_disabled: bool,
    has_llm_endpoint: bool,
) -> EgressProfile {
    if offline_mode {
        return EgressProfile::Offline;
    }
    if has_llm_endpoint && !llm_disabled {
        return EgressProfile::ByokOnly;
    }
    EgressProfile::Full
}

pub fn offline_mode_from_env_and_db(db_offline: Option<&str>) -> bool {
    offline_mode_source(db_offline).0
}

/// Returns `(enabled, source)` where source is `env`, `db`, or `off`.
///
/// Prefer `db` when the row is on — `apply_db_to_env` mirrors that into
/// `CODASAURUS_OFFLINE`, which would otherwise look like a Render env var the
/// operator never set.
pub fn offline_mode_source(db_offline: Option<&str>) -> (bool, &'static str) {
    let db_on = db_offline
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false);
    if db_on {
        return (true, "db");
    }
    let env_on = std::env::var("CODASAURUS_OFFLINE")
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false);
    if env_on {
        return (true, "env");
    }
    (false, "off")
}

pub fn health_json(profile: EgressProfile, offline: bool, llm_allowed: bool) -> serde_json::Value {
    json!({
        "egress_profile": profile.as_str(),
        "offline_mode": offline,
        "network": {
            "llm": llm_allowed && !offline,
            "registries": !offline,
            "osv": !offline,
        },
        "note": if offline {
            "Fail-closed: LLM disabled; registry/OSV network calls skipped or cache-only."
        } else {
            "BYOK LLM and Tier-1 network checks allowed subject to SSRF policy."
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_wins() {
        assert_eq!(
            resolve_egress_profile(true, false, true),
            EgressProfile::Offline
        );
    }

    #[test]
    fn health_offline_disables_network() {
        let j = health_json(EgressProfile::Offline, true, false);
        assert_eq!(j["egress_profile"], "offline");
        assert_eq!(j["network"]["llm"], false);
        assert_eq!(j["network"]["registries"], false);
        assert_eq!(j["network"]["osv"], false);
    }

    #[test]
    fn byok_when_llm_configured() {
        assert_eq!(
            resolve_egress_profile(false, false, true),
            EgressProfile::ByokOnly
        );
        assert_eq!(
            resolve_egress_profile(false, true, true),
            EgressProfile::Full
        );
        assert_eq!(
            resolve_egress_profile(false, false, false),
            EgressProfile::Full
        );
    }

    #[test]
    fn health_byok_allows_llm_when_configured() {
        let j = health_json(EgressProfile::ByokOnly, false, true);
        assert_eq!(j["network"]["llm"], true);
        assert_eq!(j["network"]["registries"], true);
    }
}
