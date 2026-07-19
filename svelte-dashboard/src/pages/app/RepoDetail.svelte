<script>
  import { onMount } from "svelte";
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

  onMount(async () => {
    try {
      const id = $params.id;
      const data = await api.get(`/api/repos/${id}`);
      repo = data;
      detectors = data.detectors ?? {};
      llmEnabled = data.llm_enabled ?? true;
    } catch (err) {
      error = err.message || "Failed to load repo";
    } finally {
      loading = false;
    }
  });

  async function handleSave() {
    saving = true;
    saveMsg = "";
    try {
      await api.put(`/api/repos/${$params.id}`, {
        detectors,
        llm_enabled: llmEnabled,
      });
      saveMsg = "Saved";
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
        <div style="margin-bottom:24px">
          <p style="font-size:13px;color:var(--text-muted)">
            {repo.owner ?? ""} / {repo.name} &middot; {repo.default_branch ?? "main"}
          </p>
        </div>

        <div class="card" style="margin-bottom:24px">
          <h3 style="font-size:16px;font-weight:600;margin-bottom:16px">Detectors</h3>

          {#each Object.entries(detectors) as [key, val]}
            <div style="display:flex;align-items:center;justify-content:space-between;padding:8px 0;border-bottom:1px solid var(--border)">
              <span style="font-size:14px">{formatLabel(key)}</span>
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

          {#if Object.keys(detectors).length === 0}
            <p style="color:var(--text-muted);font-size:13px">No detectors configured</p>
          {/if}
        </div>

        <div class="card" style="margin-bottom:24px">
          <h3 style="font-size:16px;font-weight:600;margin-bottom:16px">LLM Review</h3>
          <label class="toggle">
            <div class="toggle-track" class:on={llmEnabled} role="checkbox" aria-checked={llmEnabled}
              tabindex="0"
              onclick={() => (llmEnabled = !llmEnabled)}
              onkeydown={(e) => { if (e.key === 'Enter') llmEnabled = !llmEnabled; }}>
              <div class="toggle-knob"></div>
            </div>
            <span style="font-size:14px">Enable LLM-powered review</span>
          </label>
        </div>

        <div style="display:flex;align-items:center;gap:12px">
          <button class="primary" onclick={handleSave} disabled={saving}>
            {saving ? "Saving…" : "Save"}
          </button>
          {#if saveMsg}
            <span style="font-size:13px;color:{saveMsg === 'Saved' ? 'var(--success)' : 'var(--error)'}">{saveMsg}</span>
          {/if}
        </div>

        <button style="margin-top:16px" onclick={() => push("/app/repos")}>← Back to Repos</button>
      {/if}
    </div>
  </div>
</div>
