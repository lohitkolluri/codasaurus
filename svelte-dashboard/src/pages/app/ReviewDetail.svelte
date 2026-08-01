<script>
  import { onMount } from "svelte";
  import { api } from "../../stores/api.js";
  import Sidebar from "../../lib/Sidebar.svelte";
  import Header from "../../lib/Header.svelte";
  import LoadingSpinner from "../../lib/LoadingSpinner.svelte";
  import SeverityBadge from "../../lib/SeverityBadge.svelte";
  import ErrorState from "../../lib/ErrorState.svelte";

  import { params } from "svelte-spa-router";

  let review = $state(null);
  let allFindings = $state([]);
  let loading = $state(true);
  let error = $state("");

  let selectedFile = $state(null);

  let files = $derived.by(() => {
    const seen = new Set();
    return allFindings.filter((f) => {
      const path = f.file_path ?? f.file;
      if (!path || seen.has(path)) return false;
      seen.add(path);
      return true;
    });
  });

  let fileFindings = $derived.by(() => {
    if (!selectedFile || !allFindings.length) return [];
    return allFindings.filter((f) => f.file_path === selectedFile || f.file === selectedFile);
  });

  $effect(() => {
    const id = $params?.id;
    if (!id) return;
    loadReview(id);
  });

  async function loadReview(id) {
    loading = true;
    error = "";
    try {
      const data = await api.get(`/api/reviews/${id}`);
      review = data.review ?? null;
      allFindings = data.findings ?? [];
    } catch (err) {
      error = err.message || "Failed to load review";
    } finally {
      loading = false;
    }
  }

  function selectFile(path) {
    selectedFile = path;
  }

  function countSeverity(findings, severity) {
    return findings.filter((f) => f.severity === severity).length;
  }

  function getFindingsForFile(filePath) {
    return allFindings.filter((f) => f.file_path === filePath || f.file === filePath);
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
    } catch (err) {
      error = err.message || "Dismiss failed";
    }
  }
</script>

<div class="app-layout">
  <Sidebar />
  <div class="app-main">
    <Header title={review ? `PR #${review.pr_number ?? review.id}` : "Review"} />
    <div class="app-content" style="padding:0">
      <LoadingSpinner loading={loading} />

      {#if error}
        <div style="padding:32px">
          <ErrorState message={error} />
        </div>
      {:else if review}
        <div style="padding:24px 32px;border-bottom:1px solid var(--border)">
          <h2 style="font-size:20px;font-weight:700;margin-bottom:8px">{review.pr_title ?? `PR #${review.pr_number}`}</h2>
          <div style="display:flex;gap:12px;font-size:13px;color:var(--text-muted);align-items:center;flex-wrap:wrap">
            <span>{review.repo_full_name ?? ""}</span>
            <span class="status-badge {review.status}">{review.status}</span>
            {#if review.pr_head_sha}
              <span style="font-family:var(--font-code);font-size:12px">{review.pr_head_sha.slice(0, 7)}</span>
            {/if}
            {#if review.repo_full_name && review.pr_number}
              <a
                href={`https://github.com/${review.repo_full_name}/pull/${review.pr_number}`}
                target="_blank"
                rel="noopener noreferrer"
                style="color:var(--text-primary);text-decoration:underline"
              >View on GitHub</a>
            {/if}
          </div>
        </div>

        <div class="page-with-sidebar" style="height:auto;min-height:calc(100vh - var(--header-height) - 120px)">
          <div class="file-tree">
            {#each files as f}
              <div
                class="file-tree-item"
                class:active={selectedFile === (f.file_path ?? f.file)}
                onclick={() => selectFile(f.file_path ?? f.file)}
                role="button"
                tabindex="0"
                onkeydown={(e) => { if (e.key === 'Enter') selectFile(f.file_path ?? f.file); }}>
                <span>{f.file_path?.split("/").pop() ?? f.file}</span>
                <div class="severity-counts">
                  {#if countSeverity(getFindingsForFile(f.file_path ?? f.file), "blocking") > 0}
                    <span style="color:var(--error)">{countSeverity(getFindingsForFile(f.file_path ?? f.file), "blocking")}</span>
                  {/if}
                  {#if countSeverity(getFindingsForFile(f.file_path ?? f.file), "warning") > 0}
                    <span style="color:var(--text-primary)">{countSeverity(getFindingsForFile(f.file_path ?? f.file), "warning")}</span>
                  {/if}
                </div>
              </div>
            {/each}
            {#if files.length === 0}
              <div style="padding:16px;font-size:13px;color:var(--text-muted)">No files changed</div>
            {/if}
          </div>

          <div class="content-area" style="padding:24px 32px">
            {#if fileFindings.length === 0 && selectedFile}
              <p style="color:var(--text-muted)">No findings for this file</p>
            {:else if !selectedFile}
              <p style="color:var(--text-muted)">Select a file to view findings</p>
            {:else}
              {#each fileFindings as finding}
                <div class="finding-item">
                  <div class="finding-location">
                    {finding.file_path ?? finding.file}
                    {#if finding.line_start}
                      <span>:{finding.line_start}</span>
                    {/if}
                    <SeverityBadge severity={finding.severity ?? "info"} />
                    {#if finding.fingerprint}
                      <span style="font-family:var(--font-code);font-size:11px;color:var(--text-muted)">
                        {finding.fingerprint.includes(":") ? finding.fingerprint.split(":").pop().slice(0,12) : finding.fingerprint.slice(0,12)}
                      </span>
                    {/if}
                    <button
                      style="margin-left:auto;font-size:12px"
                      onclick={() => dismissFinding(finding)}
                    >Dismiss</button>
                  </div>
                  <div class="finding-message">{finding.message}</div>
                  {#if finding.code_snippet}
                    <div class="code-snippet">{finding.code_snippet}</div>
                  {/if}
                </div>
              {/each}
            {/if}
          </div>
        </div>
      {/if}
    </div>
  </div>
</div>
