<script>
  import { link, push, params } from "svelte-spa-router";
  import { api } from "../../stores/api.js";
  import { isMaintainer } from "../../stores/auth.js";
  import AppShell from "../../lib/AppShell.svelte";
  import LoadingSpinner from "../../lib/LoadingSpinner.svelte";
  import SeverityBadge from "../../lib/SeverityBadge.svelte";
  import ErrorState from "../../lib/ErrorState.svelte";
  import EmptyState from "../../lib/EmptyState.svelte";

  let review = $state(null);
  let summary = $state(null);
  let allFindings = $state([]);
  let loading = $state(true);
  let error = $state("");
  let canDismiss = $derived($isMaintainer);

  let selectedFile = $state(null);
  let severityFilter = $state("all");
  let detectorFilter = $state("all");

  let files = $derived.by(() => {
    const map = new Map();
    for (const f of allFindings) {
      const path = f.file_path ?? f.file;
      if (!path) continue;
      if (!map.has(path)) map.set(path, { path, blocking: 0, warning: 0, info: 0, total: 0 });
      const row = map.get(path);
      row.total += 1;
      if (f.severity === "blocking") row.blocking += 1;
      else if (f.severity === "warning") row.warning += 1;
      else row.info += 1;
    }
    return [...map.values()].sort((a, b) => b.blocking - a.blocking || b.total - a.total || a.path.localeCompare(b.path));
  });

  let detectors = $derived.by(() => {
    const set = new Set(allFindings.map((f) => f.detector).filter(Boolean));
    return [...set].sort();
  });

  let fileFindings = $derived.by(() => {
    let list = allFindings;
    if (selectedFile) {
      list = list.filter((f) => (f.file_path ?? f.file) === selectedFile);
    }
    if (severityFilter !== "all") {
      list = list.filter((f) => f.severity === severityFilter);
    }
    if (detectorFilter !== "all") {
      list = list.filter((f) => f.detector === detectorFilter);
    }
    return list;
  });

  let blocking = $derived(Number(summary?.by_severity?.blocking ?? review?.by_severity?.blocking ?? 0));
  let warning = $derived(Number(summary?.by_severity?.warning ?? review?.by_severity?.warning ?? 0));
  let info = $derived(Number(summary?.by_severity?.info ?? review?.by_severity?.info ?? 0));
  let findingCount = $derived(Number(summary?.finding_count ?? review?.finding_count ?? allFindings.length));
  let fileCount = $derived(Number(summary?.file_count ?? review?.file_count ?? files.length));

  $effect(() => {
    const id = $params?.id;
    if (!id) return;
    loadReview(id);
  });

  async function loadReview(id) {
    loading = true;
    error = "";
    selectedFile = null;
    severityFilter = "all";
    detectorFilter = "all";
    try {
      const data = await api.get(`/api/reviews/${id}`);
      review = data.review ?? null;
      summary = data.summary ?? null;
      allFindings = data.findings ?? [];
      if (allFindings.length > 0) {
        const first = files[0] ?? null;
        // files derived may lag one tick — pick from findings directly
        const path = allFindings[0]?.file_path ?? allFindings[0]?.file;
        if (path) selectedFile = path;
      }
    } catch (err) {
      error = err.message || "Failed to load review";
    } finally {
      loading = false;
    }
  }

  function selectFile(path) {
    selectedFile = path === selectedFile ? null : path;
  }

  function shortFp(finding) {
    const raw = finding.fingerprint ?? "";
    const fp = raw.includes(":") ? raw.split(":").pop() : raw;
    return fp.slice(0, 12);
  }

  function formatWhen(iso) {
    if (!iso) return "—";
    try {
      return new Date(iso).toLocaleString(undefined, {
        month: "short",
        day: "numeric",
        hour: "2-digit",
        minute: "2-digit",
      });
    } catch {
      return "—";
    }
  }

  function formatDuration(secs) {
    if (secs == null || secs < 0) return "—";
    if (secs < 60) return `${secs}s`;
    const m = Math.floor(secs / 60);
    const s = secs % 60;
    return s ? `${m}m ${s}s` : `${m}m`;
  }

  /** GitHub blob URL for the finding location (helps decide dismiss vs fix). */
  function githubPermalink(finding) {
    const repo = review?.repo_full_name;
    const sha = review?.pr_head_sha;
    const path = finding?.file_path ?? finding?.file;
    if (!repo || !path) return null;
    const line = finding?.line_start;
    if (sha) {
      const base = `https://github.com/${repo}/blob/${sha}/${path}`;
      return line ? `${base}#L${line}` : base;
    }
    if (review?.pr_number) {
      return `https://github.com/${repo}/pull/${review.pr_number}/files`;
    }
    return null;
  }

  function githubPrFilesUrl() {
    const repo = review?.repo_full_name;
    const n = review?.pr_number;
    if (!repo || !n) return null;
    return `https://github.com/${repo}/pull/${n}/files`;
  }

  async function dismissFinding(finding) {
    const raw = finding.fingerprint ?? "";
    const fp = raw.includes(":") ? raw.split(":").pop() : raw;
    if (!fp || fp.length < 8) {
      error = "Finding has no fingerprint to dismiss";
      return;
    }
    try {
      await api.post("/api/reviews/dismiss", {
        fingerprint: fp,
        detector: finding.detector,
        file: finding.file_path ?? finding.file,
        message: finding.message,
      });
      allFindings = allFindings.filter((f) => f.id !== finding.id);
      if (summary) {
        summary = {
          ...summary,
          finding_count: Math.max(0, (summary.finding_count ?? 1) - 1),
          by_severity: {
            ...(summary.by_severity || {}),
            [finding.severity]: Math.max(0, Number(summary.by_severity?.[finding.severity] ?? 1) - 1),
          },
        };
      }
    } catch (err) {
      error = err.message || "Dismiss failed";
    }
  }

  const detectorEntries = $derived.by(() => {
    const map = summary?.by_detector ?? review?.by_detector ?? {};
    return Object.entries(map).sort((a, b) => Number(b[1]) - Number(a[1]));
  });
</script>

<AppShell title="Review">
  <LoadingSpinner loading={loading} />

  {#if error}
    <div class="rd-pad">
      <ErrorState message={error} />
    </div>
  {:else if loading}
  {:else if review}
    <div class="rd-page">
      <header class="rd-hero">
        <div class="rd-hero-top">
          <a class="rd-back" href="#/app/reviews" use:link>← Reviews</a>
          {#if review.repo_full_name && review.pr_number}
            <a
              class="btn sm"
              href={`https://github.com/${review.repo_full_name}/pull/${review.pr_number}`}
              target="_blank"
              rel="noopener noreferrer"
            >Open on GitHub ↗</a>
          {/if}
        </div>
        <h1 class="page-title">{review.pr_title ?? `PR #${review.pr_number}`}</h1>
        <div class="rd-meta">
          <span class="rd-mono">{review.repo_full_name ?? ""}</span>
          <span class="status-badge {review.status}">{review.status}</span>
          {#if review.pr_number}
            <span>PR #{review.pr_number}</span>
          {/if}
          {#if review.pr_author}
            <span>@{review.pr_author}</span>
          {/if}
          {#if review.pr_head_sha}
            <span class="rd-mono">{review.pr_head_sha.slice(0, 7)}</span>
          {/if}
        </div>
        <div class="rd-branches">
          {#if review.pr_base_branch || review.pr_head_branch}
            <span class="rd-mono">{review.pr_head_branch ?? "?"}</span>
            <span aria-hidden="true">→</span>
            <span class="rd-mono">{review.pr_base_branch ?? "?"}</span>
          {/if}
          <span class="rd-muted">Started {formatWhen(review.started_at ?? review.created_at)}</span>
          {#if review.completed_at}
            <span class="rd-muted">· Done {formatWhen(review.completed_at)}</span>
          {/if}
          {#if summary?.duration_secs != null || review.duration_secs != null}
            <span class="rd-muted">· {formatDuration(summary?.duration_secs ?? review.duration_secs)}</span>
          {/if}
        </div>
      </header>

      <section class="rd-kpis" aria-label="Finding summary">
        <div class="rd-kpi">
          <span class="rd-kpi-label">Findings</span>
          <span class="rd-kpi-value">{findingCount}</span>
        </div>
        <div class="rd-kpi">
          <span class="rd-kpi-label">Blocking</span>
          <span class="rd-kpi-value tone-error">{blocking}</span>
        </div>
        <div class="rd-kpi">
          <span class="rd-kpi-label">Warning</span>
          <span class="rd-kpi-value tone-warn">{warning}</span>
        </div>
        <div class="rd-kpi">
          <span class="rd-kpi-label">Info</span>
          <span class="rd-kpi-value">{info}</span>
        </div>
        <div class="rd-kpi">
          <span class="rd-kpi-label">Files</span>
          <span class="rd-kpi-value">{fileCount}</span>
        </div>
      </section>

      {#if detectorEntries.length > 0}
        <section class="rd-detectors" aria-label="Detectors">
          <h2 class="rd-section-title">Detectors</h2>
          <div class="rd-chip-row">
            {#each detectorEntries as [name, count]}
              <button
                type="button"
                class="rd-chip"
                class:active={detectorFilter === name}
                onclick={() => (detectorFilter = detectorFilter === name ? "all" : name)}
              >{name} <strong>{count}</strong></button>
            {/each}
          </div>
        </section>
      {/if}

      {#if allFindings.length === 0}
        <div class="rd-clean">
          <EmptyState
            message="No findings on this review — clean pass."
            actionLabel="Back to reviews"
            onAction={() => push("/app/reviews")}
          />
        </div>
      {:else}
        <div class="rd-toolbar">
          <div class="filter-bar rd-filters">
            <div class="form-group">
              <label for="sev-filter">Severity</label>
              <select id="sev-filter" bind:value={severityFilter}>
                <option value="all">All</option>
                <option value="blocking">Blocking</option>
                <option value="warning">Warning</option>
                <option value="info">Info</option>
              </select>
            </div>
            <div class="form-group">
              <label for="det-filter">Detector</label>
              <select id="det-filter" bind:value={detectorFilter}>
                <option value="all">All</option>
                {#each detectors as d}
                  <option value={d}>{d}</option>
                {/each}
              </select>
            </div>
            <p class="rd-muted rd-filter-count">{fileFindings.length} shown</p>
          </div>
        </div>

        <div class="page-with-sidebar rd-split">
          <aside class="file-tree rd-files scroll-thin">
            <button
              type="button"
              class="file-tree-item quiet"
              class:active={selectedFile == null}
              onclick={() => (selectedFile = null)}
            >
              <span>All files</span>
              <span class="rd-file-count">{allFindings.length}</span>
            </button>
            {#each files as f}
              <button
                type="button"
                class="file-tree-item quiet"
                class:active={selectedFile === f.path}
                onclick={() => selectFile(f.path)}
                title={f.path}
              >
                <span class="rd-file-name">{f.path}</span>
                <div class="severity-counts">
                  {#if f.blocking > 0}<span style="color:var(--error)">{f.blocking}</span>{/if}
                  {#if f.warning > 0}<span style="color:var(--warning)">{f.warning}</span>{/if}
                  {#if f.info > 0}<span class="rd-muted">{f.info}</span>{/if}
                </div>
              </button>
            {/each}
          </aside>

          <div class="content-area rd-findings scroll-thin">
            {#if fileFindings.length === 0}
              <p class="rd-muted">No findings match these filters.</p>
            {:else}
              <p class="rd-nav-hint rd-muted">
                Open the line on GitHub to judge a dismiss — dashboard findings are summaries, not the full diff.
                {#if githubPrFilesUrl()}
                  <a href={githubPrFilesUrl()} target="_blank" rel="noopener noreferrer">PR files ↗</a>
                {/if}
              </p>
              {#each fileFindings as finding}
                <article class="finding-item rd-finding">
                  <div class="finding-location">
                    {#if githubPermalink(finding)}
                      <a
                        class="rd-finding-path"
                        href={githubPermalink(finding)}
                        target="_blank"
                        rel="noopener noreferrer"
                        title="Open this line on GitHub"
                      >{finding.file_path ?? finding.file}{#if finding.line_start}:{finding.line_start}{/if} ↗</a>
                    {:else}
                      <span class="rd-finding-path">{finding.file_path ?? finding.file}{#if finding.line_start}:{finding.line_start}{/if}</span>
                    {/if}
                    <SeverityBadge severity={finding.severity ?? "info"} />
                    <span class="rd-chip quiet">{finding.detector}</span>
                    {#if finding.fingerprint}
                      <span class="rd-mono rd-fp">{shortFp(finding)}</span>
                    {/if}
                    {#if canDismiss}
                      <button type="button" class="rd-dismiss quiet" onclick={() => dismissFinding(finding)}>Dismiss</button>
                    {/if}
                  </div>
                  <div class="finding-message">{finding.message}</div>
                  {#if finding.suggested_fix}
                    <div class="rd-suggestion">
                      <strong>Suggestion</strong>
                      <p>{finding.suggested_fix}</p>
                    </div>
                  {/if}
                  {#if finding.code_snippet}
                    <div class="code-snippet">{finding.code_snippet}</div>
                    {#if review.repo_full_name && review.pr_number && finding.fingerprint}
                      <p class="rd-fix-hint">
                        On the PR: <code>@codasaurus fix {shortFp(finding)}</code>
                        (needs allow_auto_fix + Contents Write).
                      </p>
                    {/if}
                  {/if}
                </article>
              {/each}
            {/if}
          </div>
        </div>
      {/if}
    </div>
  {/if}
</AppShell>

<style>
  .rd-page {
    display: flex;
    flex-direction: column;
    gap: var(--space-5);
    padding-bottom: var(--space-8);
  }

  .rd-pad {
    padding: var(--space-6);
  }

  .rd-hero-top {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--space-3);
    margin-bottom: var(--space-3);
  }

  .rd-back {
    font-size: var(--text-sm);
    color: var(--text-muted);
    text-decoration: none;
  }

  .rd-back:hover {
    color: var(--text-primary);
  }

  .rd-meta,
  .rd-branches {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-3);
    align-items: center;
    font-size: var(--text-sm);
    color: var(--text-muted);
    margin-top: var(--space-2);
  }

  .rd-mono {
    font-family: var(--font-mono, ui-monospace, monospace);
    font-size: 12px;
  }

  .rd-muted {
    color: var(--text-muted);
    font-size: var(--text-sm);
  }

  .rd-kpis {
    display: grid;
    grid-template-columns: repeat(5, minmax(0, 1fr));
    gap: var(--space-3);
  }

  .rd-kpi {
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    padding: var(--space-3) var(--space-4);
    background: var(--bg-primary);
  }

  .rd-kpi-label {
    display: block;
    font-size: 12px;
    color: var(--text-muted);
    margin-bottom: 4px;
  }

  .rd-kpi-value {
    font-size: 1.5rem;
    font-weight: var(--weight-semibold);
    letter-spacing: -0.02em;
  }

  .tone-error {
    color: var(--error);
  }

  .tone-warn {
    color: color-mix(in srgb, #f59e0b 85%, var(--text-primary));
  }

  .rd-section-title {
    font-size: var(--text-sm);
    font-weight: var(--weight-semibold);
    margin: 0 0 var(--space-2);
  }

  .rd-chip-row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .rd-chip {
    border: 1px solid var(--border);
    background: var(--bg-secondary);
    border-radius: var(--radius-pill);
    padding: 4px 10px;
    font-size: 12px;
    cursor: pointer;
    color: var(--text-secondary);
  }

  .rd-chip.active,
  .rd-chip:hover {
    border-color: color-mix(in srgb, var(--accent-soft) 50%, var(--border));
    color: var(--text-primary);
    background: var(--bg-secondary);
    transform: none;
    box-shadow: none;
  }

  .rd-chip.quiet {
    cursor: default;
    background: transparent;
  }

  .rd-filters {
    margin: 0;
    align-items: end;
  }

  .rd-filter-count {
    margin: 0 0 8px;
  }

  .rd-split {
    min-height: 420px;
    height: min(70vh, 720px);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    overflow: hidden;
  }

  .rd-files {
    max-height: 100%;
    overflow-y: auto;
  }

  .rd-files .file-tree-item {
    width: 100%;
    text-align: left;
    background: transparent;
    border: none;
    border-bottom: 1px solid var(--border);
    border-radius: 0;
    cursor: pointer;
    color: var(--text-primary);
  }

  .rd-files .file-tree-item:hover {
    background: var(--bg-secondary);
    color: var(--text-primary);
    transform: none;
    box-shadow: none;
  }

  .rd-files .file-tree-item.active {
    background: var(--bg-tertiary);
  }

  .rd-file-name {
    font-size: 12px;
    font-family: var(--font-mono, ui-monospace, monospace);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
    flex: 1;
  }

  .rd-file-count {
    font-size: 12px;
    color: var(--text-muted);
  }

  .rd-findings {
    padding: var(--space-4) var(--space-5);
    max-height: 100%;
    overflow-y: auto;
  }

  .rd-nav-hint {
    margin: 0 0 var(--space-4);
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
  }

  .rd-nav-hint a {
    color: var(--text-secondary);
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  .rd-nav-hint a:hover {
    color: var(--text-primary);
  }

  .rd-finding {
    margin-bottom: var(--space-4);
  }

  .rd-finding-path {
    font-family: var(--font-mono, ui-monospace, monospace);
    font-size: 12px;
    color: var(--text-primary);
    text-decoration: none;
  }

  a.rd-finding-path:hover {
    color: var(--accent-soft);
    text-decoration: underline;
  }

  .rd-fp {
    color: var(--text-muted);
  }

  .rd-dismiss {
    margin-left: auto;
    font-size: 12px;
  }

  .rd-dismiss:hover {
    color: var(--text-primary);
    background: var(--bg-tertiary);
    transform: none;
    box-shadow: none;
  }

  .rd-suggestion {
    margin-top: var(--space-2);
    padding: var(--space-3);
    border-left: 2px solid var(--accent-soft);
    background: color-mix(in srgb, var(--accent-soft) 6%, var(--bg-primary));
    font-size: var(--text-sm);
  }

  .rd-suggestion p {
    margin: 4px 0 0;
  }

  .rd-fix-hint {
    font-size: 12px;
    color: var(--text-muted);
    margin-top: 8px;
  }

  .rd-clean {
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    padding: var(--space-6);
  }

  @media (max-width: 900px) {
    .rd-kpis {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }

    .rd-split {
      display: flex;
      flex-direction: column;
    }

    .rd-files {
      max-height: 220px;
      overflow: auto;
      border-right: none;
      border-bottom: 1px solid var(--border);
    }
  }
</style>
