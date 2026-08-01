<script>
  import { onMount } from "svelte";
  import { api } from "../../stores/api.js";
  import { formatLabel } from "../../lib/utils.js";
  import Sidebar from "../../lib/Sidebar.svelte";
  import Header from "../../lib/Header.svelte";
  import LoadingSpinner from "../../lib/LoadingSpinner.svelte";
  import ErrorState from "../../lib/ErrorState.svelte";

  let settings = $state({});
  let loading = $state(true);
  let error = $state("");

  let llmProvider = $state("openrouter");
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

  /* Model search */
  let models = $state([]);
  let modelSearch = $state("");
  let modelDropdown = $state(false);
  let modelFiltered = $derived.by(() => {
    if (!modelSearch) return models.slice(0, 20);
    const q = modelSearch.toLowerCase();
    return models
      .filter(m => m.id.toLowerCase().includes(q) || m.name.toLowerCase().includes(q))
      .slice(0, 15);
  });

  const DETECTOR_KEYS = [
    "hallucinated_imports", "phantom_deps", "vulnerabilities", "secrets",
    "over_engineering", "boilerplate", "todo_leaks", "stale_api", "graph", "guidelines", "iac",
  ];

  let maxWarnings = $state("20");
  let maxBlocking = $state("0");
  let forbiddenPaths = $state("");
  let autoLabels = $state(true);
  let policySaving = $state(false);
  let policyMsg = $state("");

  const PROVIDER_DEFAULTS = {
    openrouter: { model: "openai/gpt-4o", baseUrl: "https://openrouter.ai/api/v1" },
    ollama: { model: "llama3", baseUrl: "http://localhost:11434" },
    custom: { model: "", baseUrl: "" },
    disabled: { model: "", baseUrl: "" },
  };

  async function loadModels() {
    try {
      const res = await fetch("https://openrouter.ai/api/v1/models");
      const data = await res.json();
      models = (data.data || []).map(m => ({ id: m.id, name: m.name || m.id }));
    } catch { /* offline */ }
  }

  function handleProviderChange(val) {
    llmProvider = val;
    const cfg = PROVIDER_DEFAULTS[val];
    if (cfg) { llmModel = cfg.model; llmBaseUrl = cfg.baseUrl; modelSearch = cfg.model; }
    if (val === "openrouter" && models.length === 0) loadModels();
  }

  function selectModel(m) {
    llmModel = m.id;
    modelSearch = m.id;
    modelDropdown = false;
  }

  function handleModelKeydown(e) {
    if (e.key === "Escape") { modelDropdown = false; e.target.blur(); }
  }

  function handleModelBlur() {
    setTimeout(() => (modelDropdown = false), 150);
  }

  onMount(async () => {
    try {
      const data = await api.get("/api/settings");
      settings = data;
      llmProvider = data.llm_provider ?? "openrouter";
      llmKey = data.openrouter_api_key ?? data.llm_api_key ?? "";
      llmModel = data.llm_model ?? "";
      llmBaseUrl = data.llm_base_url ?? "";
      modelSearch = llmModel;
      if (llmProvider === "openrouter") loadModels();
      const toggles = {};
      for (const key of DETECTOR_KEYS) {
        const saved = data[`${key}_enabled`];
        toggles[key] = saved !== undefined ? saved === "true" : true;
      }
      detectorToggles = toggles;
      defaultSeverity = data.default_severity ?? "warning";
      maxWarnings = data.max_warnings ?? "20";
      maxBlocking = data.max_blocking ?? "0";
      forbiddenPaths = data.forbidden_paths ?? "";
      autoLabels = data.auto_labels_enabled !== "false";
    } catch (err) {
      error = err.message || "Failed to load settings";
    } finally {
      loading = false;
    }
  });

  async function saveLLM() {
    llmSaving = true; llmMsg = "";
    try {
      const updates = [
        api.put("/api/settings/llm_provider", { value: llmProvider }),
        api.put("/api/settings/openrouter_api_key", { value: llmKey }),
        api.put("/api/settings/llm_model", { value: llmModel }),
        api.put("/api/settings/llm_base_url", { value: llmBaseUrl }),
      ];
      const results = await Promise.allSettled(updates);
      const failed = results.filter(r => r.status === "rejected");
      llmMsg = failed.length === 0 ? "Saved" : `Save failed (${failed.length} errors)`;
    } catch (err) { llmMsg = err.message || "Save failed"; }
    finally { llmSaving = false; }
  }

  async function saveDetectors() {
    detectorSaving = true; detectorMsg = "";
    try {
      const updates = Object.entries(detectorToggles).map(([key, enabled]) =>
        api.put(`/api/settings/${key}_enabled`, { value: enabled ? "true" : "false" })
      );
      const results = await Promise.allSettled(updates);
      const failed = results.filter(r => r.status === "rejected");
      detectorMsg = failed.length === 0 ? "Saved" : `Save failed (${failed.length} errors)`;
    } catch (err) { detectorMsg = err.message || "Save failed"; }
    finally { detectorSaving = false; }
  }

  async function saveSeverity() {
    severitySaving = true; severityMsg = "";
    try {
      await api.put("/api/settings/default_severity", { value: defaultSeverity });
      severityMsg = "Saved";
    } catch (err) { severityMsg = err.message || "Save failed"; }
    finally { severitySaving = false; }
  }

  async function savePolicy() {
    policySaving = true; policyMsg = "";
    try {
      const updates = [
        api.put("/api/settings/max_warnings", { value: maxWarnings }),
        api.put("/api/settings/max_blocking", { value: maxBlocking }),
        api.put("/api/settings/forbidden_paths", { value: forbiddenPaths }),
        api.put("/api/settings/auto_labels_enabled", { value: autoLabels ? "true" : "false" }),
      ];
      const results = await Promise.allSettled(updates);
      const failed = results.filter(r => r.status === "rejected");
      policyMsg = failed.length === 0 ? "Saved" : `Save failed (${failed.length} errors)`;
    } catch (err) { policyMsg = err.message || "Save failed"; }
    finally { policySaving = false; }
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
      {:else}
        <div class="card">
          <h3 class="section-heading">LLM Configuration</h3>
          <div class="form-group">
            <label for="llm-provider">Provider</label>
            <select id="llm-provider" bind:value={llmProvider} onchange={(e) => handleProviderChange(e.target.value)}>
              <option value="openrouter">OpenRouter</option>
              <option value="ollama">Ollama</option>
              <option value="custom">Custom</option>
              <option value="disabled">Disabled</option>
            </select>
          </div>
          {#if llmProvider !== "disabled"}
            <div class="form-group">
              <label for="llm-key">API Key</label>
              <input id="llm-key" type="password" bind:value={llmKey} placeholder="sk-..." />
            </div>
            <div class="form-group model-search">
              <label for="llm-model">Model</label>
              {#if llmProvider === "openrouter"}
                <div class="search-wrap">
                  <input id="llm-model" type="text" bind:value={modelSearch}
                    oninput={() => (modelDropdown = true)}
                    onfocus={() => (modelDropdown = true)}
                    onkeydown={handleModelKeydown}
                    onblur={handleModelBlur}
                    placeholder="Search models…" autocomplete="off" />
                  {#if modelDropdown && modelFiltered.length > 0}
                    <div class="search-dropdown">
                      {#each modelFiltered as m}
                        <button class="search-item" class:active={m.id === llmModel}
                          onmousedown={(e) => e.preventDefault()}
                          onclick={() => selectModel(m)}>
                          <span class="search-id">{m.id}</span>
                        </button>
                      {/each}
                    </div>
                  {/if}
                </div>
              {:else}
                <input id="llm-model" type="text" bind:value={llmModel} />
              {/if}
            </div>
            <div class="form-group">
              <label for="llm-url">Base URL</label>
              <input id="llm-url" type="text" bind:value={llmBaseUrl} />
            </div>
          {/if}
          <div class="save-row">
            <button onclick={saveLLM} disabled={llmSaving}>{llmSaving ? "Saving…" : "Save"}</button>
            {#if llmMsg}<span class="save-msg" class:error={llmMsg !== "Saved"}>{llmMsg}</span>{/if}
          </div>
        </div>

        <div class="card">
          <h3 class="section-heading">Detectors</h3>
          <div class="detector-list">
            {#each Object.entries(detectorToggles) as [key, val]}
              <div class="detector-row">
                <span>{formatLabel(key)}</span>
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
          </div>
          <div class="save-row">
            <button onclick={saveDetectors} disabled={detectorSaving}>{detectorSaving ? "Saving…" : "Save"}</button>
            {#if detectorMsg}<span class="save-msg" class:error={detectorMsg !== "Saved"}>{detectorMsg}</span>{/if}
          </div>
        </div>

        <div class="card">
          <h3 class="section-heading">Default Severity</h3>
          <div class="form-group">
            <label for="default-severity">Minimum severity to surface</label>
            <select id="default-severity" bind:value={defaultSeverity}>
              <option value="blocking">Blocking</option>
              <option value="warning">Warning</option>
              <option value="info">Info</option>
            </select>
          </div>
          <div class="save-row">
            <button onclick={saveSeverity} disabled={severitySaving}>{severitySaving ? "Saving…" : "Save"}</button>
            {#if severityMsg}<span class="save-msg" class:error={severityMsg !== "Saved"}>{severityMsg}</span>{/if}
          </div>
        </div>

        <div class="card">
          <h3 class="section-heading">Policy pack</h3>
          <div class="form-group">
            <label for="max-warnings">Max warnings (soft cap)</label>
            <input id="max-warnings" type="number" min="0" bind:value={maxWarnings} />
          </div>
          <div class="form-group">
            <label for="max-blocking">Max blocking findings</label>
            <input id="max-blocking" type="number" min="0" bind:value={maxBlocking} />
          </div>
          <div class="form-group">
            <label for="forbidden-paths">Forbidden path prefixes (comma-separated)</label>
            <input id="forbidden-paths" type="text" bind:value={forbiddenPaths} placeholder="vendor/,secrets/" />
          </div>
          <div class="detector-row" style="border:none;padding:8px 0">
            <span>Auto-apply PR labels</span>
            <label class="toggle">
              <div class="toggle-track" class:on={autoLabels} role="checkbox" aria-checked={autoLabels}
                tabindex="0"
                onclick={() => (autoLabels = !autoLabels)}
                onkeydown={(e) => { if (e.key === 'Enter') autoLabels = !autoLabels; }}>
                <div class="toggle-knob"></div>
              </div>
            </label>
          </div>
          <div class="save-row">
            <button onclick={savePolicy} disabled={policySaving}>{policySaving ? "Saving…" : "Save"}</button>
            {#if policyMsg}<span class="save-msg" class:error={policyMsg !== "Saved"}>{policyMsg}</span>{/if}
          </div>
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  .section-heading { font-size: 16px; font-weight: 600; margin-bottom: 16px; }
  .card { margin-bottom: 20px; }
  .card:last-child { margin-bottom: 0; }
  .save-row { display: flex; align-items: center; gap: 8px; margin-top: 12px; }
  .save-msg { font-size: 13px; color: var(--success); }
  .save-msg.error { color: var(--error); }
  .detector-list { max-height: 320px; overflow-y: scroll; direction: rtl; margin: 0 -24px; padding: 0 24px; scrollbar-width: thin; scrollbar-color: var(--text-muted) var(--bg-secondary); }
  .detector-list::-webkit-scrollbar { width: 6px; }
  .detector-list::-webkit-scrollbar-track { background: var(--bg-secondary); border-radius: 3px; }
  .detector-list::-webkit-scrollbar-thumb { background: var(--text-muted); border-radius: 3px; }
  .detector-row { direction: ltr; display: flex; align-items: center; justify-content: space-between; padding: 8px 0; border-bottom: 1px solid var(--border-light); font-size: 14px; }
  .detector-row:last-child { border-bottom: none; }
  .search-wrap { position: relative; }
  .search-dropdown { position: absolute; top: 100%; left: 0; right: 0; z-index: 20; max-height: 240px; overflow-y: auto; background: var(--bg-primary); border: 1px solid var(--border); border-radius: 6px; margin-top: 4px; box-shadow: var(--shadow-md); }
  .search-item { display: block; width: 100%; text-align: left; padding: 8px 12px; border: none; border-radius: 0; background: none; font-size: 13px; font-family: var(--font-mono); color: var(--text-primary); cursor: pointer; }
  .search-item:hover, .search-item.active { background: var(--bg-secondary); }
</style>
