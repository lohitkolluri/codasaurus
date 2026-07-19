<script>
  import { push } from "svelte-spa-router";
  import { api } from "../../stores/api.js";
  import Sidebar from "../../lib/Sidebar.svelte";
  import Header from "../../lib/Header.svelte";
  import LoadingSpinner from "../../lib/LoadingSpinner.svelte";
  import ErrorState from "../../lib/ErrorState.svelte";

  import { params } from "svelte-spa-router";
  import { formatLabel } from "../../lib/utils.js";

  let repo = $state(null);
  let loading = $state(true);
  let saving = $state(false);
  let error = $state("");
  let saveMsg = $state("");

  let detectors = $state({});
  let llmEnabled = $state(true);

  // Reactively load when params change (fixes $params being undefined at mount)
  $effect(() => {
    const id = $params?.id;
    if (!id) return;
    loadRepo(id);
  });

  async function loadRepo(id) {
    loading = true;
    error = "";
    try {
      const data = await api.get(`/api/repos/${id}`);
      repo = data;
      detectors = data.detectors ?? {};
      llmEnabled = data.llm_enabled ?? true;
    } catch (err) {
      error = err.message || "Failed to load repo";
    } finally {
      loading = false;
    }
  }

  async function handleSave() {
    saving = true;
    saveMsg = "";
    try {
      const id = $params?.id;
      if (!id) return;
      await api.put(`/api/repos/${id}`, {
        detectors,
        llm_enabled: llmEnabled,
      });
      saveMsg = "Saved";
      setTimeout(() => (saveMsg = ""), 2000);
    } catch (err) {
      saveMsg = err.message || "Save failed";
    } finally {
      saving = false;
    }
  }
</script>

<div class="app-layout">
  <Sidebar />
  <div class="app-main">
    <Header title={repo?.name ?? "Repository"} />
    <div class="app-content">
      <LoadingSpinner loading={loading} />

      {#if error}
        <ErrorState message={error} />
      {:else if repo}
        <div class="repo-header">
          <button class="back-btn" onclick={() => push("/app/repos")}>← Back to Repos</button>
          <h1 class="repo-name">{repo.full_name}</h1>
          <p class="repo-meta">
            <span class="branch">{repo.default_branch ?? "main"}</span>
            {#if repo.private}
              <span class="badge-private">Private</span>
            {/if}
            <span class="repo-owner">{repo.owner}</span>
          </p>
        </div>

        <div class="card">
          <h3>Detectors</h3>
          <div class="detector-grid">
            {#each Object.entries(detectors) as [key, val]}
              <div class="detector-row">
                <span class="detector-label">{formatLabel(key)}</span>
                <label class="toggle">
                  <div class="toggle-track" class:on={val ?? false} role="checkbox" aria-checked={val ?? false}
                    tabindex="0"
                    onclick={() => (detectors[key] = !(detectors[key] ?? false))}
                    onkeydown={(e) => { if (e.key === 'Enter') detectors[key] = !(detectors[key] ?? false); }}>
                    <div class="toggle-knob"></div>
                  </div>
                </label>
              </div>
            {/each}
          </div>
          {#if Object.keys(detectors).length === 0}
            <p class="empty-note">No detectors configured for this repo</p>
          {/if}
        </div>

        <div class="card">
          <h3>LLM Review</h3>
          <div class="llm-row">
            <label class="toggle">
              <div class="toggle-track" class:on={llmEnabled} role="checkbox" aria-checked={llmEnabled}
                tabindex="0"
                onclick={() => (llmEnabled = !llmEnabled)}
                onkeydown={(e) => { if (e.key === 'Enter') llmEnabled = !llmEnabled; }}>
                <div class="toggle-knob"></div>
              </div>
            </label>
            <span>Enable LLM-powered review on this repo</span>
          </div>
        </div>

        <div class="actions">
          <button class="primary" onclick={handleSave} disabled={saving}>
            {saving ? "Saving…" : "Save Changes"}
          </button>
          {#if saveMsg}
            <span class="toast" class:success={saveMsg === 'Saved'}>{saveMsg}</span>
          {/if}
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  .repo-header {
    margin-bottom: 24px;
  }
  .back-btn {
    font-size: 13px;
    color: var(--text-muted);
    background: none;
    border: none;
    cursor: pointer;
    padding: 0;
    margin-bottom: 8px;
    display: block;
  }
  .back-btn:hover {
    color: var(--accent);
  }
  .repo-name {
    font-size: 22px;
    font-weight: 700;
    margin: 0 0 6px 0;
  }
  .repo-meta {
    font-size: 13px;
    color: var(--text-muted);
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .branch {
    font-family: monospace;
    background: var(--bg-secondary);
    padding: 2px 8px;
    border-radius: 4px;
  }
  .badge-private {
    font-size: 11px;
    background: #fef3cd;
    color: #856404;
    padding: 2px 8px;
    border-radius: 4px;
  }
  .repo-owner {
    color: var(--text-muted);
  }
  .card {
    background: var(--card-bg);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 20px;
    margin-bottom: 16px;
  }
  .card h3 {
    font-size: 15px;
    font-weight: 600;
    margin: 0 0 16px 0;
  }
  .detector-grid {
    display: flex;
    flex-direction: column;
  }
  .detector-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 0;
    border-bottom: 1px solid var(--border);
  }
  .detector-row:last-child {
    border-bottom: none;
  }
  .detector-label {
    font-size: 14px;
  }
  .empty-note {
    font-size: 13px;
    color: var(--text-muted);
  }
  .llm-row {
    display: flex;
    align-items: center;
    gap: 12px;
    font-size: 14px;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .toast {
    font-size: 13px;
    padding: 4px 12px;
    border-radius: 4px;
    background: var(--bg-secondary);
  }
  .toast.success {
    color: var(--success);
    background: color-mix(in srgb, var(--success) 15%, transparent);
  }
</style>
