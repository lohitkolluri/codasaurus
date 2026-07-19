<script>
  import { onMount } from "svelte";
  import { push } from "svelte-spa-router";
  import { api } from "../../stores/api.js";
  import Sidebar from "../../lib/Sidebar.svelte";
  import Header from "../../lib/Header.svelte";
  import LoadingSpinner from "../../lib/LoadingSpinner.svelte";
  import SeverityBadge from "../../lib/SeverityBadge.svelte";
  import ErrorState from "../../lib/ErrorState.svelte";

  import { params } from "svelte-spa-router";

  let review = $state(null);
  let files = $state([]);
  let allFindings = $state([]);
  let loading = $state(true);
  let error = $state("");

  let selectedFile = $state(null);

  onMount(async () => {
    try {
      const id = $params.id;
      const data = await api.get(`/api/reviews/${id}`);
      review = data;
      files = data.files ?? [];
      allFindings = data.findings ?? [];
    } catch (err) {
      error = err.message || "Failed to load review";
    } finally {
      loading = false;
    }
  });

  let fileFindings = $derived(() => {
    if (!selectedFile || !allFindings.length) return [];
    return allFindings.filter((f) => f.file_path === selectedFile || f.file === selectedFile);
  });

  function countSeverity(findings, severity) {
    return findings.filter((f) => f.severity === severity).length;
  }

  let selectedFindings = $derived(fileFindings());

  function selectFile(filePath) {
    selectedFile = filePath;
  }

  function getFindingsForFile(filePath) {
    return allFindings.filter((f) => f.file_path === filePath || f.file === filePath);
  }
</script>

<div class="app-layout">
  <Sidebar />
  <div class="app-main">
    <Header title={review ? `Review #${review.id ?? review.pr_number}` : "Review"} />
    <div class="app-content" style="padding:0">
      <LoadingSpinner loading={loading} />

      {#if error}
        <div style="padding:32px">
          <ErrorState message={error} />
        </div>
      {:else if review}
        <div style="padding:24px 32px;border-bottom:1px solid var(--border)">
          <h2 style="font-size:20px;font-weight:700;margin-bottom:8px">{review.pr_title ?? `PR #${review.pr_number}`}</h2>
          <div style="display:flex;gap:12px;font-size:13px;color:var(--text-muted);align-items:center">
            <span>{review.repo_name ?? ""}</span>
            <span class="status-badge {review.status}">{review.status}</span>
            {#if review.commit_sha}
              <span style="font-family:var(--font-code);font-size:12px">{review.commit_sha.slice(0, 7)}</span>
            {/if}
          </div>
        </div>

        <div class="page-with-sidebar" style="height:auto;min-height:calc(100vh - var(--header-height) - 120px)">
          <div class="file-tree">
            {#each files as file}
              <div
                class="file-tree-item"
                class:active={selectedFile === (file.file_path ?? file.file)}
                onclick={() => selectFile(file.file_path ?? file.file)}
              >
                <span>{file.file_name ?? file.file_path?.split("/").pop() ?? file.file}</span>
                <div class="severity-counts">
                  {#if countSeverity(getFindingsForFile(file.file_path ?? file.file), "blocking") > 0}
                    <span style="color:var(--error)">{countSeverity(getFindingsForFile(file.file_path ?? file.file), "blocking")}</span>
                  {/if}
                  {#if countSeverity(getFindingsForFile(file.file_path ?? file.file), "warning") > 0}
                    <span style="color:var(--text-primary)">{countSeverity(getFindingsForFile(file.file_path ?? file.file), "warning")}</span>
                  {/if}
                </div>
              </div>
            {/each}
            {#if files.length === 0}
              <div style="padding:16px;font-size:13px;color:var(--text-muted)">No files changed</div>
            {/if}
          </div>

          <div class="content-area" style="padding:24px 32px">
            {#if selectedFindings.length === 0 && selectedFile}
              <p style="color:var(--text-muted)">No findings for this file</p>
            {:else if !selectedFile}
              <p style="color:var(--text-muted)">Select a file to view findings</p>
            {:else}
              {#each selectedFindings as finding}
                <div class="finding-item">
                  <div class="finding-location">
                    {finding.file_path ?? finding.file}
                    {#if finding.line_number}
                      <span>:{finding.line_number}</span>
                    {/if}
                    <SeverityBadge severity={finding.severity ?? "info"} />
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
