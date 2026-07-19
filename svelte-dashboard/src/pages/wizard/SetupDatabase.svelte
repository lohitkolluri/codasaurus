<script>
  import { push } from "svelte-spa-router";
  import { api } from "../../stores/api.js";

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
      await api.post("/api/setup/database", { provider: "postgres", url: postgresUrl });
      testResult = "Connected to PostgreSQL";
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
    <label for="pgurl">PostgreSQL Database URL</label>
    <input id="pgurl" type="text" bind:value={postgresUrl} placeholder="postgresql://user:pass@host:5432/codasaurus" />
    <div style="font-size:12px;color:var(--text-muted);margin-top:4px">
      On Render, your database URL is provided automatically via the DATABASE_URL env var.
    </div>
  </div>

  <div style="margin-bottom:16px">
    <button onclick={testConnection} disabled={testing || !postgresUrl}>
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
