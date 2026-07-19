<script>
  import { onMount } from "svelte";
  import { api } from "../../stores/api.js";
  import Sidebar from "../../lib/Sidebar.svelte";
  import Header from "../../lib/Header.svelte";
  import LoadingSpinner from "../../lib/LoadingSpinner.svelte";
  import ErrorState from "../../lib/ErrorState.svelte";

  let settings = $state({});
  let loading = $state(true);
  let error = $state("");

  let llmKey = $state("");
  let llmModel = $state("");
  let llmBaseUrl = $state("");
  let llmSaving = $state(false);
  let llmMsg = $state("");

  let detectorToggles = $state({});
  let detectorSaving = $state(false);
  let detectorMsg = $state("");

  let defaultSeverity = $state("warning");
  let severitySaving = $state(false);
  let severityMsg = $state("");

  onMount(async () => {
    try {
      const data = await api.get("/api/settings");
      settings = data;
      llmKey = data.llm?.api_key ?? "";
      llmModel = data.llm?.model ?? "";
      llmBaseUrl = data.llm?.base_url ?? "";
      detectorToggles = data.detectors ?? {};
      defaultSeverity = data.default_severity ?? "warning";
    } catch (err) {
      error = err.message || "Failed to load settings";
    } finally {
      loading = false;
    }
  });

  async function saveLLM() {
    llmSaving = true;
    llmMsg = "";
    try {
      await api.put("/api/settings/llm", {
        api_key: llmKey,
        model: llmModel,
        base_url: llmBaseUrl,
      });
      llmMsg = "Saved";
    } catch (err) {
      llmMsg = err.message || "Save failed";
    } finally {
      llmSaving = false;
    }
  }

  async function saveDetectors() {
    detectorSaving = true;
    detectorMsg = "";
    try {
      await api.put("/api/settings/detectors", { detectors: detectorToggles });
      detectorMsg = "Saved";
    } catch (err) {
      detectorMsg = err.message || "Save failed";
    } finally {
      detectorSaving = false;
    }
  }

  async function saveSeverity() {
    severitySaving = true;
    severityMsg = "";
    try {
      await api.put("/api/settings/default_severity", { severity: defaultSeverity });
      severityMsg = "Saved";
    } catch (err) {
      severityMsg = err.message || "Save failed";
    } finally {
      severitySaving = false;
    }
  }
</script>

<div class="app-layout">
  <Sidebar />
  <div class="app-main">
    <Header title="Settings" />
    <div class="app-content">
      <LoadingSpinner loading={loading} />

      {#if error}
        <ErrorState message={error} />
      {:else if loading}
        <!-- spinner -->
      {:else}
        <!-- LLM Section -->
        <div class="card" style="margin-bottom:24px">
          <h3 style="font-size:16px;font-weight:600;margin-bottom:16px">LLM Configuration</h3>
          <div class="form-group">
            <label for="llm-key">API Key</label>
            <input id="llm-key" type="password" bind:value={llmKey} placeholder="sk-..." />
          </div>
          <div class="form-group">
            <label for="llm-model">Model</label>
            <input id="llm-model" type="text" bind:value={llmModel} />
          </div>
          <div class="form-group">
            <label for="llm-url">Base URL</label>
            <input id="llm-url" type="text" bind:value={llmBaseUrl} />
          </div>
          <div style="display:flex;align-items:center;gap:8px">
            <button onclick={saveLLM} disabled={llmSaving}>{llmSaving ? "Saving…" : "Save"}</button>
            {#if llmMsg}
              <span style="font-size:13px;color:{llmMsg === 'Saved' ? 'var(--success)' : 'var(--error)'}">{llmMsg}</span>
            {/if}
          </div>
        </div>

        <!-- Detectors Section -->
        <div class="card" style="margin-bottom:24px">
          <h3 style="font-size:16px;font-weight:600;margin-bottom:16px">Detectors</h3>
          {#each Object.entries(detectorToggles) as [key, val]}
            <div style="display:flex;align-items:center;justify-content:space-between;padding:8px 0;border-bottom:1px solid var(--border)">
              <span style="font-size:14px">{key}</span>
              <label class="toggle">
                <div class="toggle-track" class:on={val ?? false} role="checkbox" aria-checked={val ?? false}
                  tabindex="0"
                  onclick={() => (detectorToggles[key] = !(detectorToggles[key] ?? false))}
                  onkeydown={(e) => { if (e.key === 'Enter') detectorToggles[key] = !(detectorToggles[key] ?? false); }}>
                  <div class="toggle-knob"></div>
                </div>
              </label>
            </div>
          {/each}
          {#if Object.keys(detectorToggles).length === 0}
            <p style="color:var(--text-muted);font-size:13px">No detectors available</p>
          {/if}
          <div style="display:flex;align-items:center;gap:8px;margin-top:12px">
            <button onclick={saveDetectors} disabled={detectorSaving}>{detectorSaving ? "Saving…" : "Save"}</button>
            {#if detectorMsg}
              <span style="font-size:13px;color:{detectorMsg === 'Saved' ? 'var(--success)' : 'var(--error)'}">{detectorMsg}</span>
            {/if}
          </div>
        </div>

        <!-- Default Severity -->
        <div class="card" style="margin-bottom:24px">
          <h3 style="font-size:16px;font-weight:600;margin-bottom:16px">Default Severity</h3>
          <div class="form-group">
            <select bind:value={defaultSeverity}>
              <option value="blocking">Blocking</option>
              <option value="warning">Warning</option>
              <option value="info">Info</option>
            </select>
          </div>
          <div style="display:flex;align-items:center;gap:8px">
            <button onclick={saveSeverity} disabled={severitySaving}>{severitySaving ? "Saving…" : "Save"}</button>
            {#if severityMsg}
              <span style="font-size:13px;color:{severityMsg === 'Saved' ? 'var(--success)' : 'var(--error)'}">{severityMsg}</span>
            {/if}
          </div>
        </div>
      {/if}
    </div>
  </div>
</div>
