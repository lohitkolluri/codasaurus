use crate::bot::WebhookPayload;
use crate::detectors::{self, Finding, Findings};
use crate::retry::{is_reqwest_error_retryable, retry_async, RetryConfig};
use crate::state::ReviewState;
use anyhow::Result;

use super::findings::{
    collect_registry_pairs, is_critical_full_file_path, merge_vulnerability_findings,
    severity_at_least,
};
use super::github::{fetch_pr_files, github_api_headers, post_or_update_comment, GITHUB_CLIENT};
use super::llm::{generate_and_post_summary, maybe_post_auto_improve};
use super::persist::save_review_to_db;
use super::reviewers::{suggest_reviewers, MAX_REVIEWER_FILES};

/// Options for a single review invocation (webhook vs slash-command).
#[derive(Debug, Clone)]
pub struct ReviewOptions {
    /// Legacy: second describe comment (kept off — walkthrough covers open).
    pub auto_describe: bool,
    /// Run LLM `review_diff` when PR size is under budget.
    pub auto_review_diff: bool,
    /// Review draft PRs (slash `/review` and similar).
    pub force_draft: bool,
    /// Bypass completed same-SHA claim (slash `@codasaurus review`).
    pub force_rereview: bool,
}

impl Default for ReviewOptions {
    fn default() -> Self {
        Self {
            auto_describe: false,
            auto_review_diff: true,
            force_draft: false,
            force_rereview: false,
        }
    }
}

pub async fn review_pr_with_options(
    token: &str,
    repo_name: &str,
    payload: &WebhookPayload,
    options: ReviewOptions,
) -> Result<()> {
    let pr = match &payload.pull_request {
        Some(p) => p,
        None => return Ok(()),
    };

    // Draft PRs: skip unless explicitly forced (slash-command path).
    let is_draft = pr["draft"].as_bool().unwrap_or(false);
    if is_draft && !options.force_draft {
        tracing::info!(repo = repo_name, "skipping draft PR");
        return Ok(());
    }

    let pr_number = pr["number"].as_i64().unwrap_or(0);
    let mut pr_title = pr["title"].as_str().unwrap_or("").to_string();
    let pr_body = pr["body"].as_str().unwrap_or("").to_string();
    let head_sha = pr["head"]["sha"].as_str().unwrap_or("");

    let pool = crate::bot::CONFIG_POOL.get();

    // Honor dashboard active toggle (single fetch; reused for config overlay below).
    let db_repo = if let Some(pool) = pool {
        crate::db::repos::get_repo_by_full_name(pool, repo_name)
            .await
            .ok()
            .flatten()
    } else {
        None
    };
    if let Some(ref repo) = db_repo {
        if !repo.active {
            tracing::info!(repo = repo_name, "repo inactive, skipping review");
            return Ok(());
        }
    }

    let state = pool.map(ReviewState::from_pool);

    // Claim SHA at start to close TOCTOU race between concurrent workers
    if !head_sha.is_empty() {
        if let Some(ref s) = state {
            let claimed = if options.force_rereview {
                s.force_claim_sha(repo_name, pr_number, head_sha).await
            } else {
                s.try_claim_sha(repo_name, pr_number, head_sha).await
            };
            match claimed {
                Ok(false) => {
                    tracing::info!(
                        repo = repo_name,
                        pr = pr_number,
                        sha = head_sha,
                        "skipping already-claimed SHA"
                    );
                    let sha_short = head_sha.get(..7).unwrap_or(head_sha);
                    let notice = format!(
                        "### Codasaurus\n\n\
                         Already reviewed `{sha_short}` — skipping duplicate work.\n\n\
                         Push a new commit, or comment `@codasaurus review` to force a re-run."
                    );
                    let auth_header = format!("Bearer {token}");
                    let client = GITHUB_CLIENT.as_ref().ok_or_else(|| {
                        anyhow::anyhow!("GitHub API client not available (failed to initialize)")
                    })?;
                    let _ = post_or_update_comment(
                        client,
                        &auth_header,
                        repo_name,
                        pr_number,
                        &notice,
                        &state,
                        "status",
                    )
                    .await;
                    return Ok(());
                }
                Ok(true) => {}
                Err(e) => tracing::warn!(error = %e, "SHA claim failed; continuing"),
            }
        }
    }

    let mut config = crate::config::Config::load_for_bot(pool).await;
    let mut repo_llm_enabled = true;
    let mut repo_flags = crate::config::RepoBotFlags::default();
    let mut repo_config_json: Option<String> = None;
    if let Some(ref repo) = db_repo {
        if let Some(ref cfg_json) = repo.config_json {
            repo_config_json = Some(cfg_json.clone());
            repo_flags = config.overlay_repo_config_json(cfg_json);
            repo_llm_enabled = repo_flags.llm_enabled;
        }
    }
    if let Some(pool) = pool {
        if let Ok(Some(v)) = crate::db::config::get_config(pool, "auto_labels_enabled").await {
            if matches!(
                v.to_ascii_lowercase().as_str(),
                "false" | "0" | "no" | "off"
            ) {
                repo_flags.auto_labels = false;
            }
        }
        // Global Settings toggle for auto-approve (repo config_json wins when true).
        if !repo_flags.auto_approve {
            if let Ok(Some(v)) = crate::db::config::get_config(pool, "auto_approve").await {
                if matches!(v.to_ascii_lowercase().as_str(), "true" | "1" | "yes" | "on") {
                    repo_flags.auto_approve = true;
                }
            }
        }
        let repo_had_title_key = repo_config_json
            .as_deref()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
            .is_some_and(|v| v.get("pr_title_fix").is_some());
        let global_title = crate::db::config::get_config(pool, "pr_title_fix")
            .await
            .ok()
            .flatten();
        repo_flags.pr_title_fix = crate::bot::title_fix::resolve_mode(
            repo_flags.pr_title_fix,
            global_title.as_deref(),
            repo_had_title_key,
        );
    }
    let mut options = options;
    if !repo_flags.auto_describe {
        options.auto_describe = false;
    }
    if !repo_flags.auto_review_diff {
        options.auto_review_diff = false;
    }

    // Offline / air-gap: fail closed. Never call LLM; skip registry prefetch.
    let offline_mode = {
        let db_off = if let Some(pool) = pool {
            if let Ok(Some(provider)) = crate::db::config::get_config(pool, "llm_provider").await {
                if provider.eq_ignore_ascii_case("disabled") {
                    repo_llm_enabled = false;
                }
            }
            crate::db::config::get_config(pool, "offline_mode")
                .await
                .ok()
                .flatten()
        } else {
            None
        };
        crate::bot::offline::offline_mode_from_env_and_db(db_off.as_deref())
    };
    if offline_mode {
        repo_llm_enabled = false;
        options.auto_review_diff = false;
        tracing::info!(
            repo = repo_name,
            "offline_mode enabled: LLM disabled, registry cache-only"
        );
    }
    crate::registry::set_offline_mode(offline_mode);

    let mut policy_pack = crate::bot::policy::PolicyPack::load(
        pool,
        repo_config_json.as_deref(),
        config.pre_merge.max_blocking,
        config.pre_merge.max_warnings,
    )
    .await;
    let strictness = {
        let from_repo = policy_pack.review_strictness.as_deref();
        if let Some(s) = from_repo {
            crate::bot::strictness::ReviewStrictness::parse(s)
        } else {
            crate::bot::strictness::load(
                pool,
                config.behavior.strict,
                config.behavior.review_strictness.as_deref(),
            )
            .await
        }
    };
    strictness.apply_to_pack(&mut policy_pack);
    tracing::info!(strictness = %strictness.as_str(), "review strictness applied");
    let policy = crate::config::BotPolicy {
        min_severity: policy_pack.min_severity.clone(),
    };
    let runtime = crate::bot_runtime::BotRuntimeConfig::default();

    // Keep local FS guidelines off; remote guidelines applied after Contents API fetch.
    let want_guidelines = config.checks.guidelines;
    config.checks.guidelines = false;

    let client = GITHUB_CLIENT
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("GitHub API client not available (failed to initialize)"))?;
    let auth_header = format!("Bearer {token}");
    let headers = github_api_headers(&auth_header)?;

    let base_sha = pr["base"]["sha"].as_str().unwrap_or("");
    let head_ref = pr["head"]["ref"].as_str().unwrap_or("");

    let files = fetch_pr_files(client, repo_name, pr_number, &auth_header)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "failed to fetch PR files");
            e
        })?;
    if files.is_empty() {
        tracing::info!(repo = repo_name, pr = pr_number, "no files in PR");
        // Avoid leaving an in_progress lease that would skip later pushes.
        complete_sha_claim(&state, repo_name, pr_number, head_sha).await;
        return Ok(());
    }

    let exclude = config.checks.exclude_patterns.clone();
    let exclude_prepared = crate::detectors::prepare_exclude_patterns(&exclude);
    let files: Vec<serde_json::Value> = files
        .into_iter()
        .filter(|f| {
            let name = f["filename"].as_str().unwrap_or("");
            !crate::detectors::is_excluded_prepared(name, &exclude_prepared)
        })
        .collect();
    if files.is_empty() {
        tracing::info!(
            repo = repo_name,
            pr = pr_number,
            "all PR files excluded by patterns"
        );
        complete_sha_claim(&state, repo_name, pr_number, head_sha).await;
        return Ok(());
    }

    if let Some(pool) = pool {
        if let Err(e) = crate::baseline::clear_pr_diff_lines(pool, repo_name, pr_number).await {
            tracing::warn!(error = %e, "clear pr_diff_lines failed");
        }
        for file in &files {
            let path = file["filename"].as_str().unwrap_or("");
            let patch = file["patch"].as_str().unwrap_or("");
            let changed = crate::baseline::parse_patch_changed_lines(patch);
            if changed.is_empty() {
                continue;
            }
            if let Err(e) =
                crate::baseline::save_pr_diff_lines(pool, repo_name, pr_number, path, &changed)
                    .await
            {
                tracing::warn!(error = %e, "save pr_diff_lines failed");
                break;
            }
        }
    }

    let changed_paths: Vec<String> = files
        .iter()
        .filter_map(|f| f["filename"].as_str().map(String::from))
        .collect();
    let low_signal_only = crate::llm::all_paths_low_signal(&changed_paths);
    if low_signal_only {
        tracing::info!(
            repo = repo_name,
            pr = pr_number,
            "low-signal PR (lockfile/vendor/generated): skipping Contents fan-out, prefetch, and LLM"
        );
    }

    let mut parsed_files_collected: Vec<crate::parser::ParsedFile> = Vec::new();
    let mut already_have = std::collections::HashSet::new();
    for file in &files {
        let filename = file["filename"].as_str().unwrap_or("unknown");
        already_have.insert(filename.to_string());
        let patch = file["patch"].as_str().unwrap_or("");
        if !patch.is_empty() && patch.len() < 100_000 {
            let parsed = match crate::parser::parse_unified_diff(filename, patch) {
                Ok(p) => Some(p),
                Err(e) => {
                    tracing::warn!(file = %filename, error = %e, "failed to parse file");
                    None
                }
            };
            if let Some(p) = parsed {
                parsed_files_collected.push(p);
            }
        }
    }

    // Full-file fetch for critical paths (auth/crypto/IaC/Docker) when under size budget.
    // Overlap with commits fetch — commits do not depend on critical file contents.
    let head_for_files = if head_sha.is_empty() {
        head_ref
    } else {
        head_sha
    };
    let critical_paths: Vec<String> = if !low_signal_only {
        let rich_paths: std::collections::HashSet<&str> = parsed_files_collected
            .iter()
            .filter(|p| p.raw_content.len() > 400)
            .map(|p| p.path.as_str())
            .collect();
        changed_paths
            .iter()
            .filter(|path| is_critical_full_file_path(path))
            .filter(|path| !rich_paths.contains(path.as_str()))
            .cloned()
            .collect()
    } else {
        Vec::new()
    };

    let pr_author = pr["user"]["login"].as_str().unwrap_or("");
    let commits_url = format!("https://api.github.com/repos/{repo_name}/pulls/{pr_number}/commits");
    let (fetched_critical, commit_messages) = tokio::join!(
        async {
            if critical_paths.is_empty() {
                Vec::new()
            } else {
                crate::bot::github_files::fetch_repo_files_parallel(
                    client,
                    &headers,
                    repo_name,
                    &critical_paths,
                    head_for_files,
                    5,
                )
                .await
            }
        },
        async {
            match retry_async(
                &RetryConfig::api_default(),
                "fetch_pr_commits",
                &is_reqwest_error_retryable,
                || async {
                    client
                        .get(&commits_url)
                        .headers(headers.clone())
                        .send()
                        .await
                        .map_err(Into::into)
                },
            )
            .await
            {
                Ok(r) => match r.json::<Vec<serde_json::Value>>().await {
                    Ok(commits) => commits
                        .iter()
                        .filter_map(|c| c["commit"]["message"].as_str().map(String::from))
                        .collect(),
                    Err(_) => vec![],
                },
                Err(_) => vec![],
            }
        },
    );
    for (path, content) in fetched_critical {
        if let Ok(parsed) = crate::parser::parse_file(&path, &content) {
            parsed_files_collected.retain(|p| p.path != path);
            parsed_files_collected.push(parsed);
        }
    }

    // Capture head-side manifest text before base-branch bootstrap overwrites.
    let mut head_manifests: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for p in &parsed_files_collected {
        if crate::bot::dep_delta::is_manifest_path(&p.path) && !p.raw_content.is_empty() {
            head_manifests.insert(p.path.clone(), p.raw_content.clone());
        }
    }
    let head_for_manifest = if head_sha.is_empty() {
        head_ref
    } else {
        head_sha
    };
    if !low_signal_only {
        let manifest_paths: Vec<String> = changed_paths
            .iter()
            .filter(|p| crate::bot::dep_delta::is_manifest_path(p))
            .filter(|path| {
                head_manifests
                    .get(*path)
                    .map(|c| c.len() < 80)
                    .unwrap_or(true)
            })
            .cloned()
            .collect();
        for (path, content) in crate::bot::github_files::fetch_repo_files_parallel(
            client,
            &headers,
            repo_name,
            &manifest_paths,
            head_for_manifest,
            5,
        )
        .await
        {
            head_manifests.insert(path, content);
        }
    }

    // Repo awareness: manifests, CONTRIBUTING/AGENTS, CODEOWNERS, linked issues.
    let (remote_ctx, bootstrapped) = if low_signal_only {
        (Default::default(), Vec::new())
    } else {
        crate::bot::repo_context::gather_remote_context(
            client,
            &headers,
            repo_name,
            base_sha,
            head_ref,
            &pr_title,
            &pr_body,
            &changed_paths,
            &already_have,
        )
        .await
        .unwrap_or_default()
    };

    let mut dep_deltas: Vec<crate::bot::dep_delta::DepDelta> = Vec::new();
    for base in &bootstrapped {
        if let Some(new_content) = head_manifests.get(&base.path) {
            if let Some(d) =
                crate::bot::dep_delta::diff_manifest(&base.path, &base.raw_content, new_content)
            {
                dep_deltas.push(d);
            }
        }
    }
    // Changed manifests skipped by bootstrap (already in the PR) still need a real base fetch.
    let bootstrapped_paths: std::collections::HashSet<&str> =
        bootstrapped.iter().map(|b| b.path.as_str()).collect();
    let changed_set: std::collections::HashSet<&str> =
        changed_paths.iter().map(String::as_str).collect();
    let base_fetch_paths: Vec<String> = head_manifests
        .keys()
        .filter(|path| !bootstrapped_paths.contains(path.as_str()))
        .filter(|path| changed_set.contains(path.as_str()))
        .cloned()
        .collect();
    let base_contents = crate::bot::github_files::fetch_repo_files_parallel(
        client,
        &headers,
        repo_name,
        &base_fetch_paths,
        base_sha,
        5,
    )
    .await;
    for (path, old_content) in base_contents {
        if let Some(new_content) = head_manifests.get(&path) {
            if let Some(d) = crate::bot::dep_delta::diff_manifest(&path, &old_content, new_content)
            {
                dep_deltas.push(d);
            }
        }
    }
    for file in &files {
        let path = file["filename"].as_str().unwrap_or("");
        if !crate::bot::dep_delta::is_manifest_path(path) {
            continue;
        }
        if dep_deltas.iter().any(|d| d.path == path) {
            continue;
        }
        let patch = file["patch"].as_str().unwrap_or("");
        if let Some(d) = crate::bot::dep_delta::delta_from_patch(path, patch) {
            dep_deltas.push(d);
        }
    }

    // Prefer full base-branch manifests over incomplete patch slices.
    for m in bootstrapped {
        parsed_files_collected.retain(|p| p.path != m.path);
        parsed_files_collected.push(m);
    }
    // Re-inject full head manifests so detectors see complete files, not hunk-only slices.
    for (path, content) in &head_manifests {
        if content.is_empty() {
            continue;
        }
        if let Ok(parsed) = crate::parser::parse_file(path, content) {
            parsed_files_collected.retain(|p| &p.path != path);
            parsed_files_collected.push(parsed);
        }
    }
    if remote_ctx.manifests_added > 0 {
        tracing::info!(
            n = remote_ctx.manifests_added,
            "bootstrapped dependency manifests from base branch"
        );
    }

    // Warm registry/OSV caches concurrently before sync detectors run.
    let prefetch_pairs = collect_registry_pairs(&parsed_files_collected);
    if !prefetch_pairs.is_empty() && !offline_mode && !low_signal_only {
        crate::registry::prefetch_packages(&prefetch_pairs).await;
    }

    // Detectors stay sync but registry hits are now mostly cache; still isolate on a worker.
    // Move parsed files into the closure (no clone) and move them back for later use.
    let mut findings = if parsed_files_collected.is_empty() {
        Findings::new()
    } else {
        let cfg = config.clone();
        let parsed = std::mem::take(&mut parsed_files_collected);
        let repo_for_learning = repo_name.to_string();
        let (out, parsed_back) = tokio::task::spawn_blocking(move || {
            let out = detectors::run_all(&parsed, &cfg, Some(repo_for_learning.as_str()));
            (out, parsed)
        })
        .await
        .map_err(|e| anyhow::anyhow!("detector task join error: {e}"))?;
        parsed_files_collected = parsed_back;
        out
    };

    if want_guidelines && !remote_ctx.guidelines.is_empty() {
        // Probe required files from guidelines (budgeted Contents GETs).
        let mut required: Vec<String> = Vec::new();
        for gf in &remote_ctx.guidelines {
            for rule in &gf.rules {
                if let crate::context::rules::ExtractedRule::FileRequired { path } = rule {
                    let p = path.trim().trim_start_matches('/').to_string();
                    if !p.is_empty() && !required.iter().any(|x| x == &p) {
                        required.push(p);
                    }
                }
            }
        }
        required.truncate(5);
        let mut present_paths = changed_paths.clone();
        let git_ref = if head_ref.is_empty() {
            if base_sha.is_empty() {
                "HEAD"
            } else {
                base_sha
            }
        } else {
            head_ref
        };
        for (path, _) in crate::bot::github_files::fetch_repo_files_parallel(
            client, &headers, repo_name, &required, git_ref, 5,
        )
        .await
        {
            present_paths.push(path);
        }

        let g = detectors::guidelines::detect_remote(
            &remote_ctx.guidelines,
            head_ref,
            &commit_messages,
            &changed_paths,
            &present_paths,
            &pr_title,
        );
        findings.findings.extend(g);
    }

    // Mine feedback, related PRs, and reviewer suggestions concurrently (independent I/O).
    let (mine_result, related_prs, mut reviewers) = tokio::join!(
        async {
            if let Some(pool) = pool {
                let store = crate::learning::store::LearningStore::from_pool(pool);
                crate::learning::mine::mine_pr_comment_feedback(
                    client, &headers, repo_name, pr_number, &store,
                )
                .await
            } else {
                Ok(0usize)
            }
        },
        crate::bot::related_prs::find_related_prs(
            client,
            &headers,
            repo_name,
            &changed_paths,
            pr_number,
        ),
        suggest_reviewers(
            client,
            &auth_header,
            repo_name,
            &files,
            pr_author,
            runtime.max_reviewer_files.min(MAX_REVIEWER_FILES),
        ),
    );
    match mine_result {
        Ok(n) if n > 0 => tracing::info!(learned = n, "mined PR feedback into rules"),
        Ok(_) => {}
        Err(e) => tracing::debug!(error = %e, "feedback mining skipped"),
    }
    let related_prs = related_prs.unwrap_or_default();
    if !related_prs.is_empty() {
        tracing::info!(n = related_prs.len(), "found related PRs");
    }

    // Collect all detector output before floors/budgets so every finding is treated equally.
    let slop_findings = crate::detectors::slop::detect_slop(
        &parsed_files_collected,
        &pr_title,
        &pr_body,
        &commit_messages,
    );
    findings.findings.extend(slop_findings);

    // Forbidden-path hits must see pre-budget volume (count caps are meaningful only then).
    findings
        .findings
        .extend(crate::bot::policy::forbidden_path_findings(
            &changed_paths,
            &policy_pack.forbidden_paths,
        ));

    findings
        .findings
        .retain(|f| severity_at_least(f.severity, &policy.min_severity));

    crate::bot::policy::enforce_count_caps(&mut findings.findings, &policy_pack);

    let agent_signal =
        crate::bot::agent_mode::detect_agent_pr(&pr_title, &pr_body, &commit_messages, pr_author);
    let mut budget = crate::bot::SignalBudget::default();
    budget = strictness.signal_budget(budget);
    if agent_signal.is_agent {
        budget = crate::bot::agent_mode::agent_signal_budget(budget);
        tracing::info!(
            reasons = ?agent_signal.reasons,
            "agent-authored PR: Tier-1 prioritized budget"
        );
    }

    // Surface highest-signal findings only; policy detector ranks near secrets.
    crate::bot::apply_signal_budget(&mut findings.findings, &budget);

    if let Some(pool) = pool {
        match crate::baseline::filter_new_code_findings(
            pool,
            repo_name,
            pr_number,
            &findings.findings,
        )
        .await
        {
            Ok(filtered) => findings.findings = filtered,
            Err(e) => tracing::warn!(error = %e, "baseline filter failed; keeping all findings"),
        }
    }

    crate::confidence::apply_base(&mut findings.findings);

    if repo_llm_enabled {
        if let Some(llm_cfg) = crate::llm::LlmConfig::from_db_or_env(pool).await {
            match crate::llm::judge_findings(&llm_cfg, &findings.findings).await {
                Ok(verdicts) => {
                    for v in verdicts {
                        if let Some(f) = findings.findings.get_mut(v.index) {
                            f.confidence = Some(v.confidence);
                            f.judge_rationale = Some(v.rationale);
                        }
                    }
                }
                Err(e) => tracing::warn!(error = %e, "LLM judge failed; keeping base confidence"),
            }
        }
    }

    if config.confidence.drop_ungrounded {
        crate::confidence::retain_grounded(&mut findings.findings);
    }

    let mut gate = crate::gates::QualityGate::from(config.quality_gate.clone());
    if let Some(raw) = repo_config_json.as_deref() {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) {
            if let Some(gj) = value.get("quality_gate") {
                if let Ok(cfg) =
                    serde_json::from_value::<crate::config::QualityGateConfig>(gj.clone())
                {
                    gate = cfg.into();
                }
            }
        }
    }
    let gate_result = crate::gates::evaluate_gate(&gate, &findings.findings);
    tracing::info!(
        repo = repo_name,
        pr = pr_number,
        gate_passed = gate_result.passed,
        gate = %gate.name,
        "quality gate evaluated"
    );

    let blast_report =
        crate::bot::blast::estimate_blast_radius(&parsed_files_collected, &changed_paths);
    let blast_md = crate::bot::blast::blast_markdown(&blast_report);
    let index_callers_md = if let Some(pool) = crate::bot::CONFIG_POOL.get() {
        crate::index::callers_markdown(pool, repo_name, &changed_paths).await
    } else {
        String::new()
    };
    let blast_md = if index_callers_md.is_empty() {
        blast_md
    } else {
        format!("{blast_md}\n{index_callers_md}")
    };
    let vuln_pkgs: Vec<String> = findings
        .findings
        .iter()
        .filter(|f| f.detector == "vulnerabilities")
        .filter_map(|f| {
            f.suggestion
                .as_ref()
                .and_then(|s| s.split('`').nth(1).map(str::to_string))
                .or_else(|| f.message.split('`').nth(1).map(str::to_string))
        })
        .collect();
    let dep_delta_md = crate::bot::dep_delta::dep_delta_markdown(&dep_deltas, &vuln_pkgs);
    let agent_badge_owned = crate::bot::agent_mode::agent_badge(&agent_signal);

    // CODEOWNERS first, then history-based (deduped).
    for owner in remote_ctx.codeowner_reviewers.iter().rev() {
        if owner != pr_author && !reviewers.iter().any(|r| r == owner) {
            reviewers.insert(0, owner.clone());
        }
    }
    reviewers.truncate(8);

    // Request CODEOWNERS / suggested reviewers via GitHub API (not just suggest in markdown).
    if policy_pack.request_reviewers && !reviewers.is_empty() {
        if let Err(e) = crate::bot::github_extra::request_pull_reviewers(
            client, &headers, repo_name, pr_number, &reviewers, pr_author,
        )
        .await
        {
            tracing::warn!(error = %e, "request_reviewers failed");
        }
    }

    let mut review_ctx = crate::bot::repo_context::to_review_context(
        &remote_ctx,
        repo_name,
        head_ref,
        &pr_title,
        &pr_body,
    );
    review_ctx.related_prs = related_prs.clone();
    if let Some(pool) = pool {
        if let Ok(Some(instr)) = crate::db::config::get_config(pool, "custom_instructions").await {
            if !instr.trim().is_empty() {
                let existing = review_ctx.repo_context.take().unwrap_or_default();
                review_ctx.repo_context =
                    Some(format!("Org custom instructions:\n{instr}\n\n{existing}"));
            }
        }
        let tone = strictness.llm_tone_hint();
        let existing = review_ctx.repo_context.take().unwrap_or_default();
        review_ctx.repo_context = Some(format!("{tone}\n\n{existing}"));
    } else {
        let tone = strictness.llm_tone_hint();
        let existing = review_ctx.repo_context.take().unwrap_or_default();
        review_ctx.repo_context = Some(format!("{tone}\n\n{existing}"));
    }
    let issue_assessment = crate::bot::issue_assessment::assess_linked_issues(
        &pr_title,
        &changed_paths,
        &review_ctx.linked_issues,
    );
    let issue_assessment_md = crate::bot::issue_assessment::assessment_markdown(&issue_assessment);

    let mut review_comments: Vec<serde_json::Value> = Vec::new();
    let mut has_blocking = false;
    let mut seen_detectors: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();

    // Merge vulnerability findings on the same (file, line) to avoid spam.
    // One package with 9 CVEs → one inline comment, not nine.
    let display_findings = merge_vulnerability_findings(&findings.findings);

    // Sort by severity: blocking first, then warning, then info
    let mut prioritized: Vec<&Finding> = display_findings.iter().collect();
    prioritized.sort_by_key(|f| match f.severity {
        "blocking" => 0,
        "warning" => 1,
        _ => 2,
    });

    for f in &prioritized {
        if f.severity == "blocking" {
            has_blocking = true;
        }
        if f.line == 0 {
            continue;
        }
        if crate::detectors::is_golden_fixture_path(&f.file) {
            continue;
        }
        if review_comments.len() >= runtime.max_inline_comments {
            break;
        }

        // Dedup: only 1 inline comment per (file, detector) pair.
        let key = (f.file.clone(), f.detector.clone());
        if !seen_detectors.insert(key) {
            continue;
        }

        let comment_body = crate::bot::markdown::inline_finding_comment(f);
        let comment = serde_json::json!({
            "path": f.file,
            "line": f.line,
            "side": "RIGHT",
            "body": comment_body,
        });
        review_comments.push(comment);
    }

    // Load prior findings before save_review_to_db so same-SHA re-reviews can delta.
    let progress = load_finding_progress(repo_name, pr_number, &findings).await;
    let advisory_draft = crate::bot::concern::is_advisory_draft(&findings.findings, has_blocking);

    let title_fix_note = maybe_fix_pr_title(
        client,
        &headers,
        repo_name,
        pr_number,
        &mut pr_title,
        &commit_messages,
        &changed_paths,
        &remote_ctx.guidelines,
        repo_flags.pr_title_fix,
    )
    .await;

    // Clean PR: overview only; optional APPROVE (merge still needs a human maintainer).
    if findings.is_empty() {
        let sequence = maybe_sequence_diagram(
            repo_llm_enabled && !low_signal_only,
            pool,
            &pr_title,
            &files,
            &changed_paths,
        )
        .await;
        post_split_review_comments(
            client,
            &auth_header,
            repo_name,
            pr_number,
            &state,
            &findings,
            false,
            &pr_title,
            &files,
            &reviewers,
            &config,
            &runtime,
            agent_badge_owned.as_deref(),
            &related_prs,
            &issue_assessment_md,
            &blast_md,
            &dep_delta_md,
            sequence.as_deref(),
            progress.as_ref(),
            false,
            title_fix_note.as_ref(),
        )
        .await?;
        if repo_flags.auto_approve {
            let sha_short = head_sha.get(..7).unwrap_or(head_sha);
            let review_body = serde_json::json!({
                "body": format!(
                    "### Codasaurus\n\nCommit `{sha_short}` looks clear on Tier-1 checks. \
                     A maintainer still needs to merge."
                ),
                "event": "APPROVE",
            });
            let review_url =
                format!("https://api.github.com/repos/{repo_name}/pulls/{pr_number}/reviews");
            match retry_async(
                &RetryConfig::api_default(),
                "post_pr_approve",
                &is_reqwest_error_retryable,
                || async {
                    client
                        .post(&review_url)
                        .header("Authorization", &auth_header)
                        .header("Accept", "application/vnd.github+json")
                        .header(
                            "User-Agent",
                            concat!("codasaurus/", env!("CARGO_PKG_VERSION")),
                        )
                        .json(&review_body)
                        .send()
                        .await
                        .map_err(Into::into)
                },
            )
            .await
            {
                Ok(resp) if resp.status().is_success() => {}
                Ok(resp) => tracing::warn!(status = %resp.status(), "auto APPROVE failed"),
                Err(e) => tracing::warn!(error = %e, "auto APPROVE failed"),
            }
        }
        if !head_sha.is_empty() {
            if let Some(ref s) = state {
                if let Err(e) = s
                    .set_reviewed_sha_async(repo_name, pr_number, head_sha)
                    .await
                {
                    tracing::warn!(error = %e, "failed to store reviewed SHA");
                };
            }
        }
        // Persist clean review to local DB
        save_review_to_db(
            repo_name,
            pr_number,
            &pr_title,
            pr["user"]["login"].as_str().unwrap_or(""),
            pr["base"]["ref"].as_str().unwrap_or(""),
            pr["head"]["ref"].as_str().unwrap_or(""),
            head_sha,
            &findings,
            false,
        )
        .await;
        if policy_pack.create_check_run {
            if let Err(e) = crate::bot::github_extra::create_findings_check_run(
                client,
                &headers,
                repo_name,
                head_sha,
                &findings.findings,
                &dep_delta_md,
                Some((&gate_result, config.quality_gate.block_on_fail)),
            )
            .await
            {
                tracing::warn!(error = %e, "check run failed");
            }
        }
        if repo_flags.auto_labels {
            let paths: Vec<String> = files
                .iter()
                .filter_map(|f| f["filename"].as_str().map(str::to_string))
                .collect();
            let labels = crate::bot::github_extra::suggest_labels(&paths, &[]);
            if let Err(e) = crate::bot::github_extra::apply_labels(
                client, &headers, repo_name, pr_number, &labels,
            )
            .await
            {
                tracing::warn!(error = %e, "auto-apply labels failed");
            }
        }
        return Ok(());
    }

    let sequence = maybe_sequence_diagram(
        repo_llm_enabled && !low_signal_only,
        pool,
        &pr_title,
        &files,
        &changed_paths,
    )
    .await;
    post_split_review_comments(
        client,
        &auth_header,
        repo_name,
        pr_number,
        &state,
        &findings,
        has_blocking,
        &pr_title,
        &files,
        &reviewers,
        &config,
        &runtime,
        agent_badge_owned.as_deref(),
        &related_prs,
        &issue_assessment_md,
        &blast_md,
        &dep_delta_md,
        sequence.as_deref(),
        progress.as_ref(),
        advisory_draft,
        title_fix_note.as_ref(),
    )
    .await?;

    // Inline findings / merge-blocking still need a Pull Request Review; keep
    // that body short so we do not spam a second full walkthrough per commit.
    // Skip the review entirely when there is nothing to attach and no block.
    // REQUEST_CHANGES only when Tier-1 blocking; LLM/soft findings stay COMMENT.
    if !review_comments.is_empty() || has_blocking {
        let sha_short = head_sha.get(..7).unwrap_or(head_sha);
        let review_event = crate::bot::concern::review_event(has_blocking, false, false);
        let summary = if has_blocking {
            format!(
                "Commit `{sha_short}`: please fix the items in the **Findings** list, \
                 then check the {} inline comment{}.",
                review_comments.len(),
                if review_comments.len() == 1 { "" } else { "s" }
            )
        } else if advisory_draft {
            format!(
                "Commit `{sha_short}`: advisory notes only (soft findings). \
                 Codasaurus is not requesting changes — {} inline comment{}.",
                review_comments.len(),
                if review_comments.len() == 1 { "" } else { "s" }
            )
        } else {
            format!(
                "Commit `{sha_short}`: a few optional notes on the Files tab \
                 ({} inline). Overview has the checklist.",
                review_comments.len()
            )
        };
        let review_body = serde_json::json!({
            "body": format!("### Codasaurus\n\n{summary}"),
            "event": review_event,
            "comments": review_comments,
        });

        let review_url =
            format!("https://api.github.com/repos/{repo_name}/pulls/{pr_number}/reviews");
        let already_posted = super::github::review_exists_for_commit(
            client,
            &auth_header,
            repo_name,
            pr_number,
            head_sha,
        )
        .await
        .unwrap_or(false);
        if already_posted {
            tracing::info!(
                repo = repo_name,
                pr_number,
                sha = head_sha,
                "PR review already exists for commit; skipping duplicate POST"
            );
        } else {
            let resp = retry_async(
                &RetryConfig::api_default(),
                "post_pr_review",
                &is_reqwest_error_retryable,
                || async {
                    client
                        .post(&review_url)
                        .header("Authorization", &auth_header)
                        .header("Accept", "application/vnd.github+json")
                        .header(
                            "User-Agent",
                            concat!("codasaurus/", env!("CARGO_PKG_VERSION")),
                        )
                        .json(&review_body)
                        .send()
                        .await
                        .map_err(Into::into)
                },
            )
            .await?;

            if !resp.status().is_success() {
                // Walkthrough already updated above; do not claim SHA so a retry can
                // re-post inline comments / REQUEST_CHANGES after transient GitHub errors.
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                tracing::warn!(
                    status = %status,
                    body = %body.chars().take(400).collect::<String>(),
                    "PR review API failed after walkthrough update"
                );
                anyhow::bail!("GitHub PR review POST failed with status {status}");
            }
        }
    }

    // Record the reviewed commit SHA for incremental review
    if !head_sha.is_empty() {
        if let Some(ref s) = state {
            if let Err(e) = s
                .set_reviewed_sha_async(repo_name, pr_number, head_sha)
                .await
            {
                tracing::warn!(error = %e, "failed to store reviewed SHA");
            };
        }
    }

    // Persist review + findings to local DB for dashboard and audit log
    save_review_to_db(
        repo_name,
        pr_number,
        &pr_title,
        pr["user"]["login"].as_str().unwrap_or(""),
        pr["base"]["ref"].as_str().unwrap_or(""),
        pr["head"]["ref"].as_str().unwrap_or(""),
        head_sha,
        &findings,
        has_blocking,
    )
    .await;

    if policy_pack.create_check_run {
        if let Err(e) = crate::bot::github_extra::create_findings_check_run(
            client,
            &headers,
            repo_name,
            head_sha,
            &findings.findings,
            &dep_delta_md,
            Some((&gate_result, config.quality_gate.block_on_fail)),
        )
        .await
        {
            tracing::warn!(error = %e, "check run failed");
        }
    }

    // Soft-apply suggested labels from paths + detectors (best-effort).
    if repo_flags.auto_labels {
        let paths: Vec<String> = files
            .iter()
            .filter_map(|f| f["filename"].as_str().map(str::to_string))
            .collect();
        let detectors: Vec<String> = findings
            .findings
            .iter()
            .map(|f| f.detector.clone())
            .collect();
        let labels = crate::bot::github_extra::suggest_labels(&paths, &detectors);
        if let Err(e) =
            crate::bot::github_extra::apply_labels(client, &headers, repo_name, pr_number, &labels)
                .await
        {
            tracing::warn!(error = %e, "auto-apply labels failed");
        }
    }

    // Generate and post LLM summary if enabled for this repo and an API key is available
    if repo_llm_enabled && !low_signal_only {
        if let Some(llm_cfg) = crate::llm::LlmConfig::from_db_or_env(pool).await {
            if let Err(e) = generate_and_post_summary(
                client,
                &auth_header,
                repo_name,
                pr_number,
                &findings,
                &llm_cfg,
                &pr_title,
                &pr_body,
                &state,
                &review_ctx,
            )
            .await
            {
                tracing::warn!(error = %e, "failed to generate LLM summary");
            }

            // Phase 1: optional review_diff when PR is under size budget + cost gates.
            if crate::llm::should_run_auto_improve(
                has_blocking,
                &changed_paths,
                options.auto_review_diff,
                files.len(),
                runtime.auto_improve_max_files,
            ) {
                if let Err(e) = maybe_post_auto_improve(
                    client,
                    &auth_header,
                    repo_name,
                    pr_number,
                    &files,
                    &llm_cfg,
                    &review_ctx,
                    &state,
                    runtime.auto_improve_max_diff_chars,
                    crate::bot::agent_mode::agent_llm_issue_cap(agent_signal.is_agent),
                )
                .await
                {
                    tracing::warn!(error = %e, "auto review_diff failed");
                }
            }
        }
    }

    // Walkthrough already covers open/push; slash `describe` is the LLM path.
    Ok(())
}

/// Diff current findings against the previous completed review for this PR.
async fn load_finding_progress(
    repo_name: &str,
    pr_number: i64,
    findings: &detectors::Findings,
) -> Option<crate::bot::markdown::FindingProgress> {
    let pool = crate::bot::CONFIG_POOL.get()?;
    let repo = crate::db::repos::get_repo_by_full_name(pool, repo_name)
        .await
        .ok()
        .flatten()?;
    let prior = crate::db::reviews::get_latest_completed_review_for_pr(pool, repo.id, pr_number)
        .await
        .ok()
        .flatten()?;
    let prior_rows = crate::db::reviews::get_findings_for_review(pool, prior.id)
        .await
        .ok()?;
    if prior_rows.is_empty() && findings.is_empty() {
        return None;
    }
    let prior: Vec<(String, String, String)> = prior_rows
        .into_iter()
        .map(|f| {
            let fp = f.fingerprint.unwrap_or_default();
            let label = crate::bot::markdown::guide_label_parts(
                &f.detector,
                &f.message,
                &f.file_path,
                f.line_start,
            );
            (fp, label, f.severity)
        })
        .collect();
    crate::bot::markdown::compute_finding_progress(&findings.findings, &prior)
}

async fn maybe_sequence_diagram(
    llm_ok: bool,
    pool: Option<&crate::db::DbPool>,
    pr_title: &str,
    files: &[serde_json::Value],
    changed_paths: &[String],
) -> Option<String> {
    if !llm_ok || !crate::bot::markdown::should_attempt_sequence_diagram(changed_paths) {
        return None;
    }
    let llm_cfg = crate::llm::LlmConfig::from_db_or_env(pool).await?;
    let files_list = changed_paths
        .iter()
        .take(40)
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    let mut diff = String::new();
    for f in files.iter().take(20) {
        let name = f["filename"].as_str().unwrap_or("?");
        let patch = f["patch"].as_str().unwrap_or("");
        if patch.is_empty() {
            continue;
        }
        use std::fmt::Write as _;
        let _ = write!(diff, "--- a/{name}\n+++ b/{name}\n{patch}\n");
        if diff.len() > 8_000 {
            break;
        }
    }
    if diff.is_empty() {
        return None;
    }
    match crate::llm::sequence_diagram_for_diff(pr_title, &files_list, &diff, &llm_cfg).await {
        Ok(raw) => crate::bot::markdown::sanitize_sequence_mermaid(&raw).map(|(m, _)| m),
        Err(e) => {
            tracing::debug!(error = %e, "sequence diagram skipped");
            None
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn maybe_fix_pr_title(
    client: &reqwest::Client,
    headers: &reqwest::header::HeaderMap,
    repo_name: &str,
    pr_number: i64,
    pr_title: &mut String,
    commit_messages: &[String],
    changed_paths: &[String],
    guidelines: &[crate::context::guidelines::GuidelineFile],
    mode: crate::config::PrTitleFixMode,
) -> Option<crate::bot::markdown::TitleFixNote> {
    use crate::config::PrTitleFixMode;
    if matches!(mode, PrTitleFixMode::Off) {
        return None;
    }
    let want_conventional = crate::bot::title_fix::guidelines_want_conventional(guidelines);
    let proposed = crate::bot::title_fix::propose_pr_title(
        pr_title,
        commit_messages,
        changed_paths,
        want_conventional,
    )?;
    match mode {
        PrTitleFixMode::Off => None,
        PrTitleFixMode::Suggest => Some(crate::bot::markdown::TitleFixNote {
            proposed: proposed.title,
            applied: false,
        }),
        PrTitleFixMode::Auto => {
            if !proposed.auto_safe {
                return Some(crate::bot::markdown::TitleFixNote {
                    proposed: proposed.title,
                    applied: false,
                });
            }
            match crate::bot::github_extra::patch_pull_request(
                client,
                headers,
                repo_name,
                pr_number,
                Some(&proposed.title),
                None,
            )
            .await
            {
                Ok(true) => {
                    *pr_title = proposed.title.clone();
                    Some(crate::bot::markdown::TitleFixNote {
                        proposed: proposed.title,
                        applied: true,
                    })
                }
                Ok(false) | Err(_) => {
                    tracing::warn!(repo = repo_name, pr_number, "PR title auto-update failed");
                    Some(crate::bot::markdown::TitleFixNote {
                        proposed: proposed.title,
                        applied: false,
                    })
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn post_split_review_comments(
    client: &reqwest::Client,
    auth_header: &str,
    repo_name: &str,
    pr_number: i64,
    state: &Option<ReviewState>,
    findings: &detectors::Findings,
    has_blocking: bool,
    pr_title: &str,
    files: &[serde_json::Value],
    reviewers: &[String],
    config: &crate::config::Config,
    runtime: &crate::bot_runtime::BotRuntimeConfig,
    agent_badge: Option<&str>,
    related_prs: &[String],
    issue_assessment_md: &str,
    blast_md: &str,
    dep_delta_md: &str,
    sequence_mermaid: Option<&str>,
    progress: Option<&crate::bot::markdown::FindingProgress>,
    advisory_draft: bool,
    title_fix: Option<&crate::bot::markdown::TitleFixNote>,
) -> Result<()> {
    let overview = crate::bot::markdown::overview_comment_body(
        findings,
        has_blocking,
        pr_title,
        files,
        runtime,
        agent_badge,
        progress,
        advisory_draft,
        title_fix,
    );
    post_or_update_comment(
        client,
        auth_header,
        repo_name,
        pr_number,
        &overview,
        state,
        "walkthrough",
    )
    .await?;

    let extras = crate::bot::markdown::WalkthroughExtras {
        related_prs,
        issue_assessment_md,
        blast_md,
        dep_delta_md,
        sequence_mermaid,
        sequence_caption: None,
    };
    if let Some(ctx) = crate::bot::markdown::context_comment_body(&extras, runtime) {
        post_or_update_comment(
            client,
            auth_header,
            repo_name,
            pr_number,
            &ctx,
            state,
            "review_context",
        )
        .await?;
    } else if let Some(s) = state {
        // Clear stale context from a prior SHA when this run has nothing to show.
        if s.get_comment_id_async(repo_name, pr_number, "review_context")
            .await
            .ok()
            .flatten()
            .is_some()
        {
            let stub = crate::bot::markdown::context_comment_stub();
            let _ = post_or_update_comment(
                client,
                auth_header,
                repo_name,
                pr_number,
                &stub,
                state,
                "review_context",
            )
            .await;
        }
    }

    let checks = crate::bot::markdown::checks_comment_body(
        findings,
        has_blocking,
        pr_title,
        files,
        reviewers,
        config,
        runtime,
    );
    post_or_update_comment(
        client,
        auth_header,
        repo_name,
        pr_number,
        &checks,
        state,
        "review_checks",
    )
    .await?;
    Ok(())
}

/// Complete a claimed SHA so empty/excluded reviews do not strand leases.
async fn complete_sha_claim(
    state: &Option<ReviewState>,
    repo_name: &str,
    pr_number: i64,
    head_sha: &str,
) {
    if head_sha.is_empty() {
        return;
    }
    if let Some(ref s) = state {
        if let Err(e) = s
            .set_reviewed_sha_async(repo_name, pr_number, head_sha)
            .await
        {
            tracing::warn!(error = %e, "failed to store reviewed SHA");
        }
    }
}
