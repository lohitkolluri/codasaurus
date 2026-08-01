<script>
  import { onMount } from "svelte";
  import { push } from "svelte-spa-router";
  import { api } from "../../stores/api.js";
  import WizardShell from "../../lib/WizardShell.svelte";

  let provider = $state("openrouter");
  let apiKey = $state("");
  let model = $state("");
  let baseUrl = $state("");
  let testing = $state(false);
  let testResult = $state("");
  let testError = $state("");
  let configured = $state(false);
  let status = $state(null);

  let models = $state([]);
  let modelSearch = $state("");
  let modelDropdown = $state(false);
  let modelFiltered = $derived.by(() => {
    if (!modelSearch) return models.slice(0, 20);
    const q = modelSearch.toLowerCase();
    return models
      .filter((m) => m.id.toLowerCase().includes(q) || m.name.toLowerCase().includes(q))
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
      models = (data.data || []).map((m) => ({ id: m.id, name: m.name || m.id }));
    } catch {
      /* offline */
    }
  }

  onMount(async () => {
    try {
      status = await api.get("/api/setup/status");
      const cfg = await api.get("/api/setup/llm");
      if (cfg.provider) provider = cfg.provider;
      if (cfg.api_key) apiKey = cfg.api_key;
      if (cfg.model) model = cfg.model;
      if (cfg.base_url) baseUrl = cfg.base_url;
      if (cfg.provider || status?.llm) {
        configured = true;
        testResult = "Saved — Tier-1 detectors always run; LLM is additive.";
      }
      modelSearch = model;
    } catch {
      /* defaults */
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

  function canSave() {
    if (provider === "disabled") return true;
    if (provider === "ollama") return true;
    if (provider === "openrouter") return !!apiKey && !apiKey.startsWith("••••");
    if (provider === "custom") return !!baseUrl;
    return false;
  }

  async function testConnection({ skipProbe = false } = {}) {
    testing = true;
    testResult = "";
    testError = "";
    try {
      if (modelSearch.trim()) model = modelSearch.trim();
      const shouldProbe = !skipProbe && provider !== "disabled";
      const path = shouldProbe ? "/api/setup/llm?test=true" : "/api/setup/llm";
      const keyToSend = apiKey.startsWith("••••") ? "" : apiKey;
      const res = await api.post(path, {
        provider,
        api_key: keyToSend,
        model,
        base_url: baseUrl,
      });
      if (shouldProbe && res?.test_passed === false) {
        testError = "Saved, but the probe failed — check key, model, or base URL.";
        configured = true;
      } else if (shouldProbe && res?.test_passed === true) {
        testResult = "Connected — LLM probe succeeded.";
        configured = true;
      } else {
        testResult =
          provider === "disabled"
            ? "LLM disabled — Tier-1 static review only. You can enable AI later in Settings."
            : "Configuration saved.";
        configured = true;
      }
      status = { ...(status ?? {}), llm: true };
    } catch (err) {
      testError = err.message || "Connection failed";
    } finally {
      testing = false;
    }
  }

  async function skipLlm() {
    provider = "disabled";
    await testConnection({ skipProbe: true });
    if (configured) push("/setup/github");
  }
</script>

<WizardShell
  current="llm"
  {status}
  title="Add AI review (optional)"
  subtitle="Tier-1 detectors (secrets, vulns, IaC, phantom deps) work with no LLM. Bring your own key when you want deeper suggestions."
>
  <div class="form-group">
    <label for="provider">Provider</label>
    <select id="provider" bind:value={provider} onchange={(e) => handleProviderChange(e.target.value)}>
      <option value="openrouter">OpenRouter — BYOK cloud models</option>
      <option value="ollama">Ollama — local models</option>
      <option value="custom">Custom OpenAI-compatible endpoint</option>
      <option value="disabled">Skip — Tier-1 only</option>
    </select>
  </div>

  {#if provider !== "disabled"}
    {#if provider !== "ollama"}
      <div class="form-group">
        <label for="apikey">API key</label>
        <input id="apikey" type="password" bind:value={apiKey} placeholder="sk-…" autocomplete="off" />
      </div>
    {/if}

    <div class="form-group">
      <label for="model">Model</label>
      {#if provider === "openrouter"}
        <div class="search-wrap">
          <input
            id="model"
            type="text"
            bind:value={modelSearch}
            oninput={() => (modelDropdown = true)}
            onfocus={() => (modelDropdown = true)}
            onkeydown={(e) => e.key === "Escape" && (modelDropdown = false)}
            onblur={() => setTimeout(() => (modelDropdown = false), 150)}
            placeholder="Search models…"
            autocomplete="off"
          />
          {#if modelDropdown && modelFiltered.length > 0}
            <div class="search-dropdown">
              {#each modelFiltered as m}
                <button
                  class="search-item"
                  class:active={m.id === model}
                  onmousedown={(e) => e.preventDefault()}
                  onclick={() => selectModel(m)}
                >
                  <span class="search-id">{m.id}</span>
                </button>
              {/each}
            </div>
          {/if}
        </div>
      {:else}
        <input id="model" type="text" bind:value={model} placeholder="model name" />
      {/if}
    </div>

    <div class="form-group">
      <label for="baseurl">Base URL</label>
      <input
        id="baseurl"
        type="text"
        bind:value={baseUrl}
        placeholder={providerConfigs[provider]?.baseUrl || "https://…"}
      />
    </div>
  {:else}
    <div class="info-box">
      You can turn on OpenRouter or Ollama anytime under <strong>Settings → LLM</strong>.
    </div>
  {/if}

  <div style="margin-bottom:8px;display:flex;align-items:center;gap:12px;flex-wrap:wrap">
    <button onclick={() => testConnection()} disabled={testing || !canSave()}>
      {testing ? "Testing…" : provider === "disabled" ? "Save & continue" : "Test & save"}
    </button>
    {#if testResult}
      <span style="color:var(--success);font-size:13px">{testResult}</span>
    {/if}
  </div>
  {#if testError}
    <div class="error-box">{testError}</div>
  {/if}

  <div class="wizard-actions">
    <button onclick={() => push("/setup/database")}>Back</button>
    {#if provider !== "disabled"}
      <button type="button" onclick={skipLlm} disabled={testing}>Skip for now</button>
    {/if}
    <button class="primary" onclick={() => push("/setup/github")} disabled={!configured}>Continue</button>
  </div>
</WizardShell>

<style>
  .search-wrap {
    position: relative;
  }
  .search-dropdown {
    position: absolute;
    top: 100%;
    left: 0;
    right: 0;
    z-index: 20;
    max-height: 240px;
    overflow-y: auto;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 6px;
    margin-top: 4px;
    box-shadow: var(--shadow-md);
  }
  .search-item {
    display: block;
    width: 100%;
    text-align: left;
    padding: 8px 12px;
    border: none;
    border-radius: 0;
    background: none;
    font-size: 13px;
    font-family: var(--font-mono);
    color: var(--text-primary);
    cursor: pointer;
  }
  .search-item:hover,
  .search-item.active {
    background: var(--bg-secondary);
  }
</style>
