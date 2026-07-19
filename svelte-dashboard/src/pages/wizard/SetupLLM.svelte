<script>
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

  const providerConfigs = {
    openrouter: { model: "openai/gpt-4o", baseUrl: "https://openrouter.ai/api/v1" },
    ollama: { model: "llama3", baseUrl: "http://localhost:11434" },
    custom: { model: "", baseUrl: "" },
    disabled: { model: "", baseUrl: "" },
  };

  function handleProviderChange(val) {
    provider = val;
    const cfg = providerConfigs[val];
    if (cfg) {
      model = cfg.model;
      baseUrl = cfg.baseUrl;
    }
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
      <input id="model" type="text" bind:value={model} placeholder={providerConfigs[provider]?.model ?? "gpt-4"} />
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
