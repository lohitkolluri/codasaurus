<script>
  import { onMount } from "svelte";
  import { push } from "svelte-spa-router";
  import { api } from "../../stores/api.js";
  import WizardShell from "../../lib/WizardShell.svelte";

  let provider = $state("sqlite");
  let postgresUrl = $state("");
  let testing = $state(false);
  let testResult = $state("");
  let testError = $state("");
  let configured = $state(false);
  let status = $state(null);

  onMount(async () => {
    try {
      status = await api.get("/api/setup/status");
      if (status?.database) {
        configured = true;
        testResult = "Already configured — you can continue.";
      }
    } catch {
      /* ignore */
    }
  });

  async function testConnection() {
    testing = true;
    testResult = "";
    testError = "";
    try {
      const body =
        provider === "sqlite"
          ? { provider: "sqlite", url: "codasaurus.db" }
          : { provider: "postgres", url: postgresUrl };
      await api.post("/api/setup/database", body);
      testResult =
        provider === "sqlite"
          ? "SQLite ready — reviews and settings will persist here."
          : "Postgres URL validated. Runtime still uses the server’s configured database.";
      configured = true;
      status = { ...(status ?? {}), database: true };
    } catch (err) {
      testError = err.message || "Connection failed";
      configured = false;
    } finally {
      testing = false;
    }
  }
</script>

<WizardShell
  current="database"
  {status}
  title="Choose where data lives"
  subtitle="SQLite is zero-config and production-ready with a volume. Prefer it unless you already run Postgres HA."
>
  <div class="form-group">
    <label>Database</label>
    <div class="radio-card" class:selected={provider === "sqlite"}>
      <label>
        <input type="radio" name="provider" value="sqlite" bind:group={provider} />
        SQLite — recommended
      </label>
      <div class="radio-hint">Embedded file DB. Path: <code>/data/codasaurus.db</code> (or local <code>codasaurus.db</code>).</div>
    </div>
    <div class="radio-card" class:selected={provider === "postgres"}>
      <label>
        <input type="radio" name="provider" value="postgres" bind:group={provider} />
        PostgreSQL
      </label>
      <div class="radio-hint">
        Set <code>DATABASE_URL</code> when starting the server for HA. This step validates the URL and stores preference.
      </div>
    </div>
  </div>

  {#if provider === "postgres"}
    <div class="form-group">
      <label for="pgurl">Database URL</label>
      <input
        id="pgurl"
        type="text"
        bind:value={postgresUrl}
        placeholder="postgresql://user:pass@localhost:5432/codasaurus"
        autocomplete="off"
      />
    </div>
  {/if}

  <div style="margin-bottom:8px;display:flex;align-items:center;gap:12px;flex-wrap:wrap">
    <button onclick={testConnection} disabled={testing || (provider === "postgres" && !postgresUrl)}>
      {testing ? "Testing…" : configured ? "Re-test & save" : "Save & continue"}
    </button>
    {#if testResult}
      <span style="color:var(--success);font-size:13px">{testResult}</span>
    {/if}
  </div>
  {#if testError}
    <div class="error-box">{testError}</div>
  {/if}

  <div class="wizard-actions">
    <button onclick={() => push("/setup")}>Back</button>
    <button class="primary" onclick={() => push("/setup/llm")} disabled={!configured}>Continue</button>
  </div>
</WizardShell>
