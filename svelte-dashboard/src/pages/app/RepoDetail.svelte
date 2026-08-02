<script>
  import { push } from "svelte-spa-router";
  import { api } from "../../stores/api.js";
  import AppShell from "../../lib/AppShell.svelte";
  import LoadingSpinner from "../../lib/LoadingSpinner.svelte";
  import ErrorState from "../../lib/ErrorState.svelte";

  import { params } from "svelte-spa-router";
  import { formatLabel } from "../../lib/utils.js";

  let repo = $state(null);
  let loading = $state(true);
  let saving = $state(false);
  let deleting = $state(false);
  let confirmDelete = $state(false);
  let error = $state("");
  let saveMsg = $state("");

  let detectors = $state({});
  let llmEnabled = $state(true);
  let autoDescribe = $state(true);
  let autoReviewDiff = $state(false);
  let autoLabels = $state(true);
  let updatePrDescription = $state(false);
  let allowAutoFix = $state(false);
  let excludePatterns = $state("");

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
      const [data, settings] = await Promise.all([
        api.get(`/api/repos/${id}`),
        api.get("/api/settings"),
      ]);
      repo = data;
      let cfg = {};
      try {
        cfg = data.config_json ? JSON.parse(data.config_json) : {};
      } catch {
        cfg = {};
      }
      if (cfg.detectors && Object.keys(cfg.detectors).length > 0) {
        detectors = cfg.detectors;
      } else {
        const DETECTOR_KEYS = [
          "hallucinated_imports", "phantom_deps", "vulnerabilities", "secrets",
          "over_engineering", "boilerplate", "todo_leaks", "stale_api", "risky_patterns", "graph", "guidelines", "iac",
        ];
        const defaults = {};
        for (const key of DETECTOR_KEYS) {
          defaults[key] = settings[`${key}_enabled`] !== "false";
        }
        detectors = defaults;
      }
      llmEnabled = cfg.llm_enabled ?? true;
      autoDescribe = cfg.auto_describe ?? true;
      autoReviewDiff = cfg.auto_review_diff ?? false;
      autoLabels = cfg.auto_labels ?? true;
      updatePrDescription = cfg.update_pr_description ?? false;
      allowAutoFix = cfg.allow_auto_fix ?? false;
      excludePatterns = Array.isArray(cfg.exclude_patterns)
        ? cfg.exclude_patterns.join(", ")
        : (cfg.exclude_patterns ?? "");
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
        config_json: JSON.stringify({
          detectors,
          llm_enabled: llmEnabled,
          auto_describe: autoDescribe,
          auto_review_diff: autoReviewDiff,
          auto_labels: autoLabels,
          update_pr_description: updatePrDescription,
          allow_auto_fix: allowAutoFix,
          exclude_patterns: excludePatterns
            .split(/[,\n]/)
            .map((s) => s.trim())
            .filter(Boolean),
        }),
        active: repo?.active ?? true,
      });
      saveMsg = "Saved";
      setTimeout(() => (saveMsg = ""), 2000);
    } catch (err) {
      saveMsg = err.message || "Save failed";
    } finally {
      saving = false;
    }
  }

  async function handleDelete() {
    const id = $params?.id;
    if (!id) return;
    deleting = true;
    saveMsg = "";
    try {
      await api.delete(`/api/repos/${id}`);
      push("/app/repos");
    } catch (err) {
      saveMsg = err.message || "Delete failed";
      confirmDelete = false;
    } finally {
      deleting = false;
    }
  }
</script>

<AppShell title={repo?.name ?? "Repository"}>
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

    <div class="card static">
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

    <div class="card static">
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

    <div class="card static">
      <h3>Automation</h3>
      <div class="llm-row">
        <label class="toggle">
          <div class="toggle-track" class:on={autoDescribe} role="checkbox" aria-checked={autoDescribe}
            tabindex="0" onclick={() => (autoDescribe = !autoDescribe)}
            onkeydown={(e) => { if (e.key === 'Enter') autoDescribe = !autoDescribe; }}>
            <div class="toggle-knob"></div>
          </div>
        </label>
        <span>Auto-describe on open</span>
      </div>
      <div class="llm-row">
        <label class="toggle">
          <div class="toggle-track" class:on={autoReviewDiff} role="checkbox" aria-checked={autoReviewDiff}
            tabindex="0" onclick={() => (autoReviewDiff = !autoReviewDiff)}
            onkeydown={(e) => { if (e.key === 'Enter') autoReviewDiff = !autoReviewDiff; }}>
            <div class="toggle-knob"></div>
          </div>
        </label>
        <span>Auto LLM review_diff (opt-in · costly)</span>
      </div>
      <div class="llm-row">
        <label class="toggle">
          <div class="toggle-track" class:on={autoLabels} role="checkbox" aria-checked={autoLabels}
            tabindex="0" onclick={() => (autoLabels = !autoLabels)}
            onkeydown={(e) => { if (e.key === 'Enter') autoLabels = !autoLabels; }}>
            <div class="toggle-knob"></div>
          </div>
        </label>
        <span>Auto-apply labels</span>
      </div>
      <div class="llm-row">
        <label class="toggle">
          <div class="toggle-track" class:on={updatePrDescription} role="checkbox" aria-checked={updatePrDescription}
            tabindex="0" onclick={() => (updatePrDescription = !updatePrDescription)}
            onkeydown={(e) => { if (e.key === 'Enter') updatePrDescription = !updatePrDescription; }}>
            <div class="toggle-knob"></div>
          </div>
        </label>
        <span>Update PR description on describe</span>
      </div>
      <div class="llm-row">
        <label class="toggle">
          <div class="toggle-track" class:on={allowAutoFix} role="checkbox" aria-checked={allowAutoFix}
            tabindex="0" onclick={() => (allowAutoFix = !allowAutoFix)}
            onkeydown={(e) => { if (e.key === 'Enter') allowAutoFix = !allowAutoFix; }}>
            <div class="toggle-knob"></div>
          </div>
        </label>
        <span>Allow @codasaurus fix</span>
      </div>
      <div class="form-group" style="margin-top:12px">
        <label for="repo-exclude">Exclude patterns</label>
        <input id="repo-exclude" type="text" bind:value={excludePatterns} placeholder="vendor/,packages/legacy/" />
      </div>
    </div>

    <div class="actions">
      <button class="primary" onclick={handleSave} disabled={saving || deleting}>
        {saving ? "Saving…" : "Save Changes"}
      </button>
      {#if !confirmDelete}
        <button class="danger" type="button" onclick={() => (confirmDelete = true)} disabled={deleting}>
          Remove from Codasaurus
        </button>
      {:else}
        <button class="danger" type="button" onclick={handleDelete} disabled={deleting}>
          {deleting ? "Removing…" : "Confirm remove"}
        </button>
        <button type="button" onclick={() => (confirmDelete = false)} disabled={deleting}>Cancel</button>
      {/if}
      {#if saveMsg}
        <span class="toast" class:success={saveMsg === 'Saved'}>{saveMsg}</span>
      {/if}
    </div>
    <p class="delete-note">
      Removes this repo and its local review history from Codasaurus. Does not uninstall the GitHub App.
    </p>
  {/if}
</AppShell>

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
    background: var(--bg-primary);
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
    max-height: 320px;
    overflow-y: auto;
    scrollbar-width: thin;
    scrollbar-color: var(--text-muted) var(--bg-secondary);
  }
  .detector-grid::-webkit-scrollbar {
    width: 6px;
  }
  .detector-grid::-webkit-scrollbar-track {
    background: var(--bg-secondary);
    border-radius: 3px;
  }
  .detector-grid::-webkit-scrollbar-thumb {
    background: var(--text-muted);
    border-radius: 3px;
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
  .delete-note {
    font-size: 12px;
    color: var(--text-muted);
    margin: 8px 0 0;
    max-width: 36rem;
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
