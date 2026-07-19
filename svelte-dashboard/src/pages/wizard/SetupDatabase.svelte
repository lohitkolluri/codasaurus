<script>
  import { push } from "svelte-spa-router";
  import { api } from "../../stores/api.js";

  let provider = $state("sqlite");
  let postgresUrl = $state("");
  let testing = $state(false);
  let testResult = $state("");
  let testError = $state("");
  let configured = $state(false);

  async function testConnection() {
    testing = true;
    testResult = "";
    testError = "";
    try {
      const body = provider === "sqlite"
        ? { provider: "sqlite", url: "codasaurus.db" }
        : { provider: "postgres", url: postgresUrl };
      await api.post("/api/setup/database", body);
      testResult = "Connection successful";
      configured = true;
    } catch (err) {
      testError = err.message || "Connection failed";
      configured = false;
    } finally {
      testing = false;
    }
  }

  function handleNext() {
    push("/setup/llm");
  }
</script>

<div class="wizard-card">
  <div class="step-indicator">
    <span class="step-dot completed"></span>
    <span class="step-dot active"></span>
    <span class="step-dot"></span>
    <span class="step-dot"></span>
  </div>
  <p class="wizard-step-label">Step 1 of 4 — Database</p>

  <div class="form-group">
    <label>Database Provider</label>
    <div class="radio-card" class:selected={provider === "sqlite"}>
      <label>
        <input type="radio" name="provider" value="sqlite" bind:group={provider} />
        SQLite
      </label>
      <div class="radio-hint">Embedded database, no setup required. Database path: codasaurus.db</div>
    </div>
    <div class="radio-card" class:selected={provider === "postgres"}>
      <label>
        <input type="radio" name="provider" value="postgres" bind:group={provider} />
        PostgreSQL
      </label>
      <div class="radio-hint">Requires a running PostgreSQL instance</div>
    </div>
  </div>

  {#if provider === "postgres"}
    <div class="form-group">
      <label for="pgurl">Database URL</label>
      <input id="pgurl" type="text" bind:value={postgresUrl} placeholder="postgresql://user:pass@localhost:5432/codasaurus" />
    </div>
  {/if}

  <div style="margin-bottom:16px">
    <button onclick={testConnection} disabled={testing || (provider === "postgres" && !postgresUrl)}>
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
    <button onclick={() => push("/setup")}>Back</button>
    <button class="primary" onclick={handleNext} disabled={!configured}>Next Step</button>
  </div>
</div>
