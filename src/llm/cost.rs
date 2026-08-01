//! LLM cost controls: skip gates, path filters, and spend estimates.

/// Paths that rarely need LLM review (generated / lock / vendor noise).
pub fn is_low_signal_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase().replace('\\', "/");
    let file = lower.rsplit('/').next().unwrap_or(lower.as_str());

    // Lockfiles & package metadata churn
    if matches!(
        file,
        "package-lock.json"
            | "yarn.lock"
            | "pnpm-lock.yaml"
            | "bun.lock"
            | "bun.lockb"
            | "cargo.lock"
            | "poetry.lock"
            | "composer.lock"
            | "gemfile.lock"
            | "go.sum"
            | "pipfile.lock"
            | "uv.lock"
    ) {
        return true;
    }

    // Generated / build artifacts
    if lower.contains("/dist/")
        || lower.contains("/build/")
        || lower.contains("/.next/")
        || lower.contains("/target/")
        || lower.contains("/vendor/")
        || lower.contains("/node_modules/")
        || lower.contains("/__generated__/")
        || lower.contains("/generated/")
        || lower.ends_with(".min.js")
        || lower.ends_with(".min.css")
        || lower.ends_with(".map")
        || lower.ends_with(".pb.go")
        || lower.ends_with("_pb2.py")
        || lower.ends_with(".snap")
    {
        return true;
    }

    // Binary / media
    if lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".gif")
        || lower.ends_with(".webp")
        || lower.ends_with(".ico")
        || lower.ends_with(".pdf")
        || lower.ends_with(".woff")
        || lower.ends_with(".woff2")
        || lower.ends_with(".ttf")
    {
        return true;
    }

    false
}

/// True when every changed path is low-signal (skip auto LLM improve).
pub fn all_paths_low_signal(paths: &[String]) -> bool {
    !paths.is_empty() && paths.iter().all(|p| is_low_signal_path(p))
}

/// Whether auto `review_diff` should run after Tier-1.
pub fn should_run_auto_improve(
    has_blocking_tier1: bool,
    changed_paths: &[String],
    auto_review_diff_enabled: bool,
    file_count: usize,
    max_files: usize,
) -> bool {
    if !auto_review_diff_enabled {
        return false;
    }
    if file_count == 0 || file_count > max_files {
        return false;
    }
    // Tier-1 already owns the hold — skip expensive LLM nits.
    if has_blocking_tier1 {
        tracing::info!("skipping auto review_diff: blocking Tier-1 findings present");
        return false;
    }
    if all_paths_low_signal(changed_paths) {
        tracing::info!("skipping auto review_diff: all changed paths are low-signal");
        return false;
    }
    true
}

/// Filter GitHub file objects to those worth sending to the LLM.
pub fn filter_llm_files(files: &[serde_json::Value]) -> Vec<&serde_json::Value> {
    files
        .iter()
        .filter(|f| {
            let name = f["filename"].as_str().unwrap_or("");
            !name.is_empty() && !is_low_signal_path(name) && !f["patch"].as_str().unwrap_or("").is_empty()
        })
        .collect()
}

/// Rough USD microdollar estimate (1_000_000 = $1) from prompt size + output budget.
/// Uses OpenRouter-ish list prices for strong vs cheap tiers — observability only.
pub fn estimate_spend_microdollars(prompt_chars: usize, max_out_tokens: u32, strong: bool) -> u64 {
    let in_tokens = (prompt_chars as f64 / 4.0).max(1.0);
    // Assume ~40% of the output budget is used on average.
    let out_tokens = max_out_tokens as f64 * 0.4;
    let (in_per_m, out_per_m) = if strong {
        (3.0_f64, 15.0_f64) // Sonnet-class
    } else {
        (0.15_f64, 0.60_f64) // mini / flash class
    };
    let usd = (in_tokens / 1_000_000.0) * in_per_m + (out_tokens / 1_000_000.0) * out_per_m;
    (usd * 1_000_000.0).round().max(1.0) as u64
}

/// Default cheap model when operators only set a strong primary model.
pub fn default_cheap_model(primary: &str) -> String {
    let p = primary.to_ascii_lowercase();
    if p.contains("claude") || p.contains("sonnet") || p.contains("opus") {
        return "anthropic/claude-haiku-4.5".into();
    }
    if p.contains("gpt-4") || p.contains("o1") || p.contains("o3") {
        return "openai/gpt-4o-mini".into();
    }
    if p.contains(":free") || p.contains("qwen") || p.contains("haiku") || p.contains("mini") {
        return primary.to_string();
    }
    "openai/gpt-4o-mini".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lockfile_is_low_signal() {
        assert!(is_low_signal_path("package-lock.json"));
        assert!(is_low_signal_path("src/Cargo.lock"));
        assert!(!is_low_signal_path("src/main.rs"));
    }

    #[test]
    fn skip_when_blocking() {
        assert!(!should_run_auto_improve(
            true,
            &["src/a.rs".into()],
            true,
            1,
            40
        ));
    }

    #[test]
    fn skip_when_all_lockfiles() {
        assert!(!should_run_auto_improve(
            false,
            &["yarn.lock".into(), "pnpm-lock.yaml".into()],
            true,
            2,
            40
        ));
    }

    #[test]
    fn run_when_code_changed() {
        assert!(should_run_auto_improve(
            false,
            &["src/lib.rs".into()],
            true,
            1,
            40
        ));
    }

    #[test]
    fn cheap_model_for_sonnet() {
        let m = default_cheap_model("anthropic/claude-sonnet-4.6");
        assert!(m.contains("haiku") || m.contains("mini"));
    }
}
