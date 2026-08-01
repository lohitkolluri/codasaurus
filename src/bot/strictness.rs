//! Review strictness presets — maps to severity floor, signal budgets, and LLM tone.

use crate::bot::quality::SignalBudget;
use crate::bot::policy::PolicyPack;

/// Org-level review personality (Settings + `.codasaurus.toml`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReviewStrictness {
    /// Hide info-level nits; tighter warning surface.
    Lenient,
    /// Default floors/budgets from policy settings.
    #[default]
    Balanced,
    /// Keep more warnings; slightly wider surface than balanced.
    Strict,
    /// Surface info findings and style nits aggressively.
    Nitpick,
}

impl ReviewStrictness {
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "lenient" | "quiet" | "minimal" => Self::Lenient,
            "strict" => Self::Strict,
            "nitpick" | "pedantic" | "noisy" => Self::Nitpick,
            _ => Self::Balanced,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lenient => "lenient",
            Self::Balanced => "balanced",
            Self::Strict => "strict",
            Self::Nitpick => "nitpick",
        }
    }

    /// Adjust policy pack floors/caps for this preset.
    pub fn apply_to_pack(self, pack: &mut PolicyPack) {
        match self {
            Self::Lenient => {
                // Hide info unless the org already demands blocking-only.
                if pack.min_severity == "info" {
                    pack.min_severity = "warning".into();
                }
                pack.max_warnings = pack.max_warnings.min(12);
            }
            Self::Balanced => {}
            Self::Strict => {
                if pack.min_severity == "blocking" {
                    pack.min_severity = "warning".into();
                }
                pack.max_warnings = pack.max_warnings.max(15);
            }
            Self::Nitpick => {
                pack.min_severity = "info".into();
                pack.max_warnings = pack.max_warnings.max(40);
            }
        }
    }

    /// Signal budget overlay (how many inline findings of each severity).
    pub fn signal_budget(self, base: SignalBudget) -> SignalBudget {
        match self {
            Self::Lenient => SignalBudget {
                max_blocking: base.max_blocking,
                max_warning: base.max_warning.min(5),
                max_info: 0,
            },
            Self::Balanced => base,
            Self::Strict => SignalBudget {
                max_blocking: base.max_blocking,
                max_warning: base.max_warning.max(12),
                max_info: base.max_info.max(2).min(4),
            },
            Self::Nitpick => SignalBudget {
                max_blocking: base.max_blocking,
                max_warning: base.max_warning.max(20),
                max_info: base.max_info.max(15),
            },
        }
    }

    /// Appended to LLM custom instructions when present.
    pub fn llm_tone_hint(self) -> &'static str {
        match self {
            Self::Lenient => {
                "Review tone: lenient. Prefer only high-confidence, merge-blocking issues. Skip style and naming nits."
            }
            Self::Balanced => {
                "Review tone: balanced. Report clear bugs and risks; skip pure preference nits."
            }
            Self::Strict => {
                "Review tone: strict. Flag correctness, security, and maintainability issues thoroughly."
            }
            Self::Nitpick => {
                "Review tone: nitpick. Also report style, naming, and small clarity issues when confident."
            }
        }
    }
}

/// Load strictness from DB (`review_strictness`) with TOML `behavior.strict` fallback.
pub async fn load(
    pool: Option<&crate::db::DbPool>,
    toml_strict: bool,
    toml_strictness: Option<&str>,
) -> ReviewStrictness {
    if let Some(pool) = pool {
        if let Ok(Some(v)) = crate::db::config::get_config(pool, "review_strictness").await {
            return ReviewStrictness::parse(&v);
        }
    }
    if let Some(s) = toml_strictness {
        if !s.trim().is_empty() {
            return ReviewStrictness::parse(s);
        }
    }
    if toml_strict {
        return ReviewStrictness::Strict;
    }
    ReviewStrictness::Balanced
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_aliases() {
        assert_eq!(ReviewStrictness::parse("NITPICK"), ReviewStrictness::Nitpick);
        assert_eq!(ReviewStrictness::parse("quiet"), ReviewStrictness::Lenient);
        assert_eq!(ReviewStrictness::parse(""), ReviewStrictness::Balanced);
    }

    #[test]
    fn lenient_raises_floor() {
        let mut pack = PolicyPack {
            min_severity: "info".into(),
            max_warnings: 50,
            ..Default::default()
        };
        ReviewStrictness::Lenient.apply_to_pack(&mut pack);
        assert_eq!(pack.min_severity, "warning");
        assert!(pack.max_warnings <= 12);
    }

    #[test]
    fn nitpick_forces_info() {
        let mut pack = PolicyPack {
            min_severity: "warning".into(),
            ..Default::default()
        };
        ReviewStrictness::Nitpick.apply_to_pack(&mut pack);
        assert_eq!(pack.min_severity, "info");
    }
}
