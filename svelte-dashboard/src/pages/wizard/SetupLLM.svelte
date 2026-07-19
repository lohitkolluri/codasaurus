<script>
  import { onMount } from "svelte";
  import { push } from "svelte-spa-router";
  import { api } from "../../stores/api.js";

  let provider = $state("openrouter");
  let apiKey = $state("");
  let model = $state("");
  let baseUrl = $state("");
  let testing = $state(false);
  let testResult = $state("");
  let testError = $state("");
  let configured = $state(false);

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

  const providerConfigs = {
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

  onMount(async () => {
    try {
      const cfg = await api.get("/api/setup/llm");
      if (cfg.provider) provider = cfg.provider;
      if (cfg.api_key) apiKey = cfg.api_key;
      if (cfg.model) model = cfg.model;
      if (cfg.base_url) baseUrl = cfg.base_url;
      if (cfg.provider) configured = true;
      modelSearch = model;
    } catch {
      // no saved config — use defaults
    }
    if (provider === "openrouter") loadModels();
  });

  function handleProviderChange(val) {
    provider = val;
    const cfg = providerConfigs[val];
    if (cfg) {
      model = cfg.model;
      baseUrl = cfg.baseUrl;
      modelSearch = cfg.model;
    }
    if (val === "openrouter" && models.length === 0) loadModels();
  }

  function selectModel(m) {
    model = m.id;
    modelSearch = m.id;
    modelDropdown = false;
  }

  function handleModelKeydown(e) {
    if (e.key === "Escape") { modelDropdown = false; e.target.blur(); }
  }

  function handleModelBlur() {
    setTimeout(() => (modelDropdown = false), 150);
  }

  async function testConnection() {
    testing = true;
    testResult = "";
    testError = "";
    try {
      await api.post("/api/setup/llm", { provider, api_key: apiKey, model, base_url: baseUrl });
      testResult = "Connection successful";
      configured = true;
    } catch (err) {
      testError = err.message || "Connection failed";
    } finally {
      testing = false;
    }
  }

  function handleNext() {
    push("/setup/github");
  }
</script>

<div class="wizard-card">
  <div class="step-indicator">
    <span class="step-dot completed"></span>
    <span class="step-dot completed"></span>
    <span class="step-dot active"></span>
    <span class="step-dot"></span>
  </div>
  <p class="wizard-step-label">Step 2 of 4 — LLM Configuration</p>

  <div class="form-group">
    <label for="provider">LLM Provider</label>
    <select id="provider" bind:value={provider} onchange={(e) => handleProviderChange(e.target.value)}>
      <option value="openrouter">OpenRouter</option>
      <option value="ollama">Ollama</option>
      <option value="custom">Custom</option>
      <option value="disabled">Disabled</option>
    </select>
  </div>

  {#if provider !== "disabled"}
    <div class="form-group">
      <label for="apikey">API Key</label>
      <input id="apikey" type="password" bind:value={apiKey} placeholder="sk-..." />
    </div>

    <div class="form-group">
      <label for="model">Model</label>
      {#if provider === "openrouter"}
        <div class="search-wrap">
          <input id="model" type="text" bind:value={modelSearch}
            oninput={() => (modelDropdown = true)}
            onfocus={() => (modelDropdown = true)}
            onkeydown={handleModelKeydown}
            onblur={handleModelBlur}
            placeholder="Search models…" autocomplete="off" />
          {#if modelDropdown && modelFiltered.length > 0}
            <div class="search-dropdown">
              {#each modelFiltered as m}
                <button class="search-item" class:active={m.id === model}
                  onmousedown={(e) => e.preventDefault()}
                  onclick={() => selectModel(m)}>
                  <span class="search-id">{m.id}</span>
                </button>
              {/each}
            </div>
          {/if}
        </div>
      {:else}
        <input id="model" type="text" bind:value={model} />
      {/if}
    </div>

    <div class="form-group">
      <label for="baseurl">Base URL</label>
      <input id="baseurl" type="text" bind:value={baseUrl} placeholder={providerConfigs[provider]?.baseUrl ?? "https://"} />
    </div>
  {/if}

  <div style="margin-bottom:16px">
    <button onclick={testConnection} disabled={testing || (provider !== "disabled" && !apiKey)}>
      {testing ? "Testing…" : "Test & Save"}
    </button>
    {#if testResult}
      <span style="color:var(--success);margin-left:12px;font-size:13px">{testResult}</span>
    {/if}
    {#if testError}
      <span class="error-state" style="display:block;padding:12px 0;text-align:left">{testError}</span>
    {/if}
  </div>

  <div class="wizard-actions">
    <button onclick={() => push("/setup/database")}>Back</button>
    <button class="primary" onclick={handleNext} disabled={!configured}>Next Step</button>
  </div>
</div>

<style>
  .search-wrap { position: relative; }
  .search-dropdown { position: absolute; top: 100%; left: 0; right: 0; z-index: 20; max-height: 240px; overflow-y: auto; background: var(--bg-primary); border: 1px solid var(--border); border-radius: 6px; margin-top: 4px; box-shadow: var(--shadow-md); }
  .search-item { display: block; width: 100%; text-align: left; padding: 8px 12px; border: none; border-radius: 0; background: none; font-size: 13px; font-family: var(--font-mono); color: var(--text-primary); cursor: pointer; }
  .search-item:hover, .search-item.active { background: var(--bg-secondary); }
</style>
