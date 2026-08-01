use crate::bot::WebhookPayload;
use crate::detectors::{self, Finding, Findings};
use crate::retry::{is_reqwest_error_retryable, retry_async, RetryConfig};
use crate::state::ReviewState;
use anyhow::Result;

use super::findings::{
    collect_registry_pairs, is_critical_full_file_path, merge_vulnerability_findings,
    severity_at_least,
};
use super::github::{
    fetch_pr_files, github_api_headers, post_or_update_comment, GITHUB_CLIENT,
};
use super::llm::{generate_and_post_summary, maybe_post_auto_improve};
use super::persist::save_review_to_db;
use super::reviewers::{suggest_reviewers, MAX_REVIEWER_FILES};

/// Options for a single review invocation (webhook vs slash-command).
#[derive(Debug, Clone, Default)]
pub struct ReviewOptions {
    /// Post a describe-style comment (typically on opened / reopened).
    pub auto_describe: bool,
    /// Run LLM `review_diff` when PR size is under budget.
    pub auto_review_diff: bool,
}

pub async fn review_pr(token: &str, repo_name: &str, payload: &WebhookPayload) -> Result<()> {
    review_pr_with_options(token, repo_name, payload, ReviewOptions::default()).await
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

    // Draft PRs: skip full review (edge case) unless explicitly forced via comment path
    let is_draft = pr["draft"].as_bool().unwrap_or(false);
    if is_draft {
        tracing::info!(repo = repo_name, "skipping draft PR");
        return Ok(());
    }

    let pr_number = pr["number"].as_i64().unwrap_or(0);
    let pr_title = pr["title"].as_str().unwrap_or("").to_string();
    let pr_body = pr["body"].as_str().unwrap_or("").to_string();
    let head_sha = pr["head"]["sha"].as_str().unwrap_or("");

    let pool = crate::bot::CONFIG_POOL.get();

    // Honor dashboard active toggle
    if let Some(pool) = pool {
        if let Ok(Some(repo)) = crate::db::repos::get_repo_by_full_name(pool, repo_name).await {
            if !repo.active {
                tracing::info!(repo = repo_name, "repo inactive — skipping review");
                return Ok(());
            }
        }
    }

    let state = pool
        .map(ReviewState::from_pool)
        .or_else(|| ReviewState::open().ok());

    // Claim SHA at start to close TOCTOU race between concurrent workers
    if !head_sha.is_empty() {
        if let Some(ref s) = state {
            match s.try_claim_sha(repo_name, pr_number, head_sha).await {
                Ok(false) => {
                    tracing::info!(repo = repo_name, pr = pr_number, sha = head_sha, "skipping already-claimed SHA");
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
    if let Some(pool) = pool {
        if let Ok(Some(repo)) = crate::db::repos::get_repo_by_full_name(pool, repo_name).await {
            if let Some(ref cfg_json) = repo.config_json {
                repo_config_json = Some(cfg_json.clone());
                repo_flags = config.overlay_repo_config_json(cfg_json);
                repo_llm_enabled = repo_flags.llm_enabled;
            }
        }
        if let Ok(Some(v)) = crate::db::config::get_config(pool, "auto_labels_enabled").await {
            if matches!(v.to_ascii_lowercase().as_str(), "false" | "0" | "no" | "off") {
                repo_flags.auto_labels = false;
            }
        }
    }
    let mut options = options;
    if !repo_flags.auto_describe {
        options.auto_describe = false;
    }
    if !repo_flags.auto_review_diff {
        options.auto_review_diff = false;
    }

    // Offline / air-gap: fail closed — never call LLM; skip registry prefetch.
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
        tracing::info!(repo = repo_name, "offline_mode enabled — LLM disabled, registry cache-only");
    }
    crate::registry::set_offline_mode(offline_mode);

    let policy_pack = crate::bot::policy::PolicyPack::load(
        pool,
        repo_config_json.as_deref(),
        config.pre_merge.max_blocking,
        config.pre_merge.max_warnings,
    )
    .await;
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
        return Ok(());
    }

    let exclude = config.checks.exclude_patterns.clone();
    let files: Vec<serde_json::Value> = files
        .into_iter()
        .filter(|f| {
            let name = f["filename"].as_str().unwrap_or("");
            !crate::detectors::is_excluded(name, &exclude)
        })
        .collect();
    if files.is_empty() {
        tracing::info!(repo = repo_name, pr = pr_number, "all PR files excluded by patterns");
        return Ok(());
    }

    let changed_paths: Vec<String> = files
        .iter()
        .filter_map(|f| f["filename"].as_str().map(String::from))
        .collect();

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
                    eprintln!("Warning: failed to parse file {filename}: {e}");
                    None
                }
            };
            if let Some(p) = parsed {
                parsed_files_collected.push(p);
            }
        }
    }

    // Full-file fetch for critical paths (auth/crypto/IaC/Docker) when under size budget.
    let head_for_files = if head_sha.is_empty() { head_ref } else { head_sha };
    for path in &changed_paths {
        if !is_critical_full_file_path(path) {
            continue;
        }
        if parsed_files_collected
            .iter()
            .any(|p| p.path == *path && p.raw_content.len() > 400)
        {
            continue;
        }
        match crate::bot::github_files::fetch_repo_file(client, &headers, repo_name, path, head_for_files)
            .await
        {
            Ok(Some(content)) if !content.is_empty() => {
                if let Ok(parsed) = crate::parser::parse_file(path, &content) {
                    parsed_files_collected.retain(|p| p.path != *path);
                    parsed_files_collected.push(parsed);
                }
            }
            _ => {}
        }
    }

    // Fetch commits early — used by slop + remote guidelines.
    let pr_author = pr["user"]["login"].as_str().unwrap_or("");
    let commits_url = format!("https://api.github.com/repos/{repo_name}/pulls/{pr_number}/commits");
    let commit_messages: Vec<String> = match retry_async(
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
    };

    // Capture head-side manifest text before base-branch bootstrap overwrites.
    let mut head_manifests: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for p in &parsed_files_collected {
        if crate::bot::dep_delta::is_manifest_path(&p.path) && !p.raw_content.is_empty() {
            head_manifests.insert(p.path.clone(), p.raw_content.clone());
        }
    }
    let head_for_manifest = if head_sha.is_empty() { head_ref } else { head_sha };
    for path in changed_paths
        .iter()
        .filter(|p| crate::bot::dep_delta::is_manifest_path(p))
    {
        let need_fetch = head_manifests
            .get(path)
            .map(|c| c.len() < 80)
            .unwrap_or(true);
        if !need_fetch {
            continue;
        }
        if let Ok(Some(content)) = crate::bot::github_files::fetch_repo_file(
            client,
            &headers,
            repo_name,
            path,
            head_for_manifest,
        )
        .await
        {
            if !content.is_empty() {
                head_manifests.insert(path.clone(), content);
            }
        }
    }

    // Repo awareness: manifests, CONTRIBUTING/AGENTS, CODEOWNERS, linked issues.
    let (remote_ctx, bootstrapped) = crate::bot::repo_context::gather_remote_context(
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
    .unwrap_or_default();

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
    for (path, new_content) in &head_manifests {
        if bootstrapped.iter().any(|b| &b.path == path) {
            continue;
        }
        if !changed_paths.iter().any(|p| p == path) {
            continue;
        }
        let old_content = crate::bot::github_files::fetch_repo_file(
            client,
            &headers,
            repo_name,
            path,
            base_sha,
        )
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
        if let Some(d) = crate::bot::dep_delta::diff_manifest(path, &old_content, new_content) {
            dep_deltas.push(d);
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
    if !prefetch_pairs.is_empty() && !offline_mode {
        crate::registry::prefetch_packages(&prefetch_pairs).await;
    }

    // Detectors stay sync but registry hits are now mostly cache; still isolate on a worker.
    let mut findings = if parsed_files_collected.is_empty() {
        Findings::new()
    } else {
        let cfg = config.clone();
        let parsed = parsed_files_collected.clone();
        tokio::task::spawn_blocking(move || detectors::run_all(&parsed, &cfg))
            .await
            .map_err(|e| anyhow::anyhow!("detector task join error: {e}"))?
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
        for path in &required {
            match crate::bot::github_files::fetch_repo_file(
                client, &headers, repo_name, path, git_ref,
            )
            .await
            {
                Ok(Some(_)) => present_paths.push(path.clone()),
                Ok(None) => {}
                Err(e) => tracing::debug!(error = %e, path, "required-file probe failed"),
            }
        }

        let g = detectors::guidelines::detect_remote(
            &remote_ctx.guidelines,
            head_ref,
            &commit_messages,
            &changed_paths,
            &present_paths,
        );
        findings.findings.extend(g);
    }

    // Mine human feedback on this PR into learned rules (budgeted; best-effort).
    if let Some(pool) = pool {
        let store = crate::learning::store::LearningStore::from_pool(pool);
        match crate::learning::mine::mine_pr_comment_feedback(
            client, &headers, repo_name, pr_number, &store,
        )
        .await
        {
            Ok(n) if n > 0 => tracing::info!(learned = n, "mined PR feedback into rules"),
            Ok(_) => {}
            Err(e) => tracing::debug!(error = %e, "feedback mining skipped"),
        }
    }

    // Related PRs by path history (for LLM / walkthrough context).
    let related_prs = crate::bot::related_prs::find_related_prs(
        client,
        &headers,
        repo_name,
        &changed_paths,
        pr_number,
    )
    .await
    .unwrap_or_default();
    if !related_prs.is_empty() {
        tracing::info!(n = related_prs.len(), "found related PRs");
    }

    // Apply default_severity floor from dashboard settings
    findings
        .findings
        .retain(|f| severity_at_least(f.severity, &policy.min_severity));

    let slop_findings = crate::detectors::slop::detect_slop(
        &parsed_files_collected,
        &pr_title,
        &pr_body,
        &commit_messages,
    );
    findings.findings.extend(slop_findings);

    let agent_signal = crate::bot::agent_mode::detect_agent_pr(
        &pr_title,
        &pr_body,
        &commit_messages,
        pr_author,
    );
    let mut budget = crate::bot::SignalBudget::default();
    if agent_signal.is_agent {
        budget = crate::bot::agent_mode::agent_signal_budget(budget);
        tracing::info!(
            reasons = ?agent_signal.reasons,
            "agent-authored PR — Tier-1 prioritized budget"
        );
    }

    // Org-scale: severity budgets + noise ranking (high-signal only).
    crate::bot::apply_signal_budget(&mut findings.findings, &budget);

    let blast_report = crate::bot::blast::estimate_blast_radius(
        &parsed_files_collected,
        &changed_paths,
    );
    let blast_md = crate::bot::blast::blast_markdown(&blast_report);
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

    // Policy pack: forbidden paths + count caps.
    findings.findings.extend(crate::bot::policy::forbidden_path_findings(
        &changed_paths,
        &policy_pack.forbidden_paths,
    ));
    crate::bot::policy::enforce_count_caps(&mut findings.findings, &policy_pack);

    let mut reviewers = suggest_reviewers(
        client,
        &auth_header,
        repo_name,
        &files,
        pr_author,
        runtime.max_reviewer_files.min(MAX_REVIEWER_FILES),
    )
    .await;
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
            client,
            &headers,
            repo_name,
            pr_number,
            &reviewers,
            pr_author,
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
                review_ctx.repo_context = Some(format!(
                    "Org custom instructions:\n{instr}\n\n{existing}"
                ));
            }
        }
    }
    let issue_assessment = crate::bot::issue_assessment::assess_linked_issues(
        &pr_title,
        &changed_paths,
        &review_ctx.linked_issues,
    );
    let issue_assessment_md =
        crate::bot::issue_assessment::assessment_markdown(&issue_assessment);

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

    // Only approve when there are genuinely no findings.
    if findings.is_empty() {
        let body = crate::bot::markdown::clean_approve_body_ext(
            agent_badge_owned.as_deref(),
            &blast_md,
            &dep_delta_md,
        );
        let review = serde_json::json!({"body": body, "event": "APPROVE"});
        let approve_url =
            format!("https://api.github.com/repos/{repo_name}/pulls/{pr_number}/reviews");
        let _: serde_json::Value = retry_async(
            &RetryConfig::api_default(),
            "approve_review",
            &is_reqwest_error_retryable,
            || async {
                client
                    .post(&approve_url)
                    .header("Authorization", &auth_header)
                    .header("Accept", "application/vnd.github+json")
                    .header(
                        "User-Agent",
                        concat!("codasaurus/", env!("CARGO_PKG_VERSION")),
                    )
                    .json(&review)
                    .send()
                    .await?
                    .error_for_status()?
                    .json()
                    .await
                    .map_err(Into::into)
            },
        )
        .await?;
        if !head_sha.is_empty() {
            if let Some(ref s) = state {
                if let Err(e) = s.set_reviewed_sha_async(repo_name, pr_number, head_sha).await {
                    eprintln!("Warning: failed to store reviewed SHA: {e}");
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
                false,
                &dep_delta_md,
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
            if let Err(e) =
                crate::bot::github_extra::apply_labels(client, &headers, repo_name, pr_number, &labels)
                    .await
            {
                tracing::warn!(error = %e, "auto-apply labels failed");
            }
        }
        return Ok(());
    }

    let walkthrough_extras = crate::bot::markdown::WalkthroughExtras {
        related_prs: &related_prs,
        issue_assessment_md: &issue_assessment_md,
        agent_badge: agent_badge_owned.as_deref(),
        blast_md: &blast_md,
        dep_delta_md: &dep_delta_md,
    };

    let body = crate::bot::markdown::walkthrough_body_ext(
        &findings,
        has_blocking,
        &pr_title,
        &files,
        &reviewers,
        &config,
        &runtime,
        false,
        repo_llm_enabled,
        walkthrough_extras,
    );

    // Try to create a review with inline comments; fall back to single comment
    let review_body = serde_json::json!({
        "body": body,
        "event": if has_blocking { "REQUEST_CHANGES" } else { "COMMENT" },
        "comments": review_comments,
    });

    let review_url = format!("https://api.github.com/repos/{repo_name}/pulls/{pr_number}/reviews");
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

    // If inline review failed (e.g. line numbers don't match), fall back to a single issue comment.
    // Uses the state store to update the previous comment rather than posting a new one.
    if !resp.status().is_success() {
        post_or_update_comment(
            client,
            &auth_header,
            repo_name,
            pr_number,
            &body,
            &state,
            "walkthrough",
        )
        .await?;
    }

    // Record the reviewed commit SHA for incremental review
    if !head_sha.is_empty() {
        if let Some(ref s) = state {
            if let Err(e) = s.set_reviewed_sha_async(repo_name, pr_number, head_sha).await {
                eprintln!("Warning: failed to store reviewed SHA: {e}");
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
            has_blocking,
            &dep_delta_md,
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
            crate::bot::github_extra::apply_labels(client, &headers, repo_name, pr_number, &labels).await
        {
            tracing::warn!(error = %e, "auto-apply labels failed");
        }
    }

    // Generate and post LLM summary if enabled for this repo and an API key is available
    if repo_llm_enabled {
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

            // Phase 1: optional review_diff when PR is under size budget.
            if options.auto_review_diff && files.len() <= runtime.auto_improve_max_files {
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

    // Phase 1: auto-describe on opened/reopened (separate comment slot).
    if options.auto_describe {
        let describe = crate::bot::markdown::walkthrough_body_ext(
            &findings,
            has_blocking,
            &pr_title,
            &files,
            &reviewers,
            &config,
            &runtime,
            true,
            repo_llm_enabled,
            crate::bot::markdown::WalkthroughExtras {
                related_prs: &related_prs,
                issue_assessment_md: &issue_assessment_md,
                agent_badge: agent_badge_owned.as_deref(),
                blast_md: &blast_md,
                dep_delta_md: &dep_delta_md,
            },
        );
        let describe_body = describe.replacen(
            "### Codasaurus review",
            "### Codasaurus describe",
            1,
        );
        if let Err(e) = post_or_update_comment(
            client,
            &auth_header,
            repo_name,
            pr_number,
            &describe_body,
            &state,
            "describe",
        )
        .await
        {
            tracing::warn!(error = %e, "auto-describe comment failed");
        }
    }

    Ok(())
}
