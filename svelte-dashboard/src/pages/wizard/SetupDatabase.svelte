<script>
  import { onMount } from "svelte";
  import { push } from "svelte-spa-router";
  import { api } from "../../stores/api.js";
  import WizardShell from "../../lib/WizardShell.svelte";

  let info = $state(null);
  let postgresUrl = $state("");
  let showAdvanced = $state(false);
  let testing = $state(false);
  let testResult = $state("");
  let testError = $state("");
  let configured = $state(false);
  let status = $state(null);
  let loading = $state(true);

  onMount(async () => {
    try {
      status = await api.get("/api/setup/status");
      info = await api.get("/api/setup/database");
      if (status?.database || info?.connected) {
        configured = true;
        testResult = info?.connected
          ? `Connected to ${info.host} / ${info.database}`
          : "Already configured. You can continue.";
      }
    } catch (err) {
      testError = err.message || "Could not reach Postgres";
    } finally {
      loading = false;
    }
  });

  async function confirmConnection() {
    testing = true;
    testResult = "";
    testError = "";
    try {
      const body = {
        provider: "postgres",
        url: showAdvanced && postgresUrl ? postgresUrl : "",
      };
      const res = await api.post("/api/setup/database", body);
      testResult = res?.message || "PostgreSQL ready.";
      configured = true;
      status = { ...(status ?? {}), database: true };
      try {
        info = await api.get("/api/setup/database");
      } catch {
        /* ignore */
      }
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
  title="PostgreSQL is ready"
  subtitle="Codasaurus uses one Postgres database for reviews, jobs, sessions, and learning. Compose starts it for you."
>
  {#if loading}
    <p style="color:var(--text-muted);margin:24px 0">Checking database…</p>
  {:else}
    {#if info?.connected}
      <div class="db-status" role="status">
        <div class="db-status-row"><span>Status</span><strong>Connected</strong></div>
        <div class="db-status-row"><span>Host</span><code>{info.host}</code></div>
        <div class="db-status-row"><span>Database</span><code>{info.database}</code></div>
        {#if info.server_version}
          <div class="db-status-row"><span>Server</span><code>{info.server_version}</code></div>
        {/if}
      </div>
    {/if}

    <p class="wizard-hint" style="margin:16px 0">
      Runtime always uses <code>DATABASE_URL</code> on the Codasaurus process.
      Change it in Compose or your host env, then restart. Not mid-wizard.
    </p>

    <button
      type="button"
      class="linkish"
      onclick={() => (showAdvanced = !showAdvanced)}
      style="margin-bottom:12px"
    >
      {showAdvanced ? "Hide" : "Validate"} an alternate URL (optional)
    </button>

    {#if showAdvanced}
      <div class="form-group">
        <label for="pgurl">Postgres URL preference</label>
        <input
          id="pgurl"
          type="text"
          bind:value={postgresUrl}
          placeholder="postgresql://user:pass@localhost:5432/codasaurus"
          autocomplete="off"
        />
        <div class="radio-hint" style="margin-top:8px">
          Validates connectivity and stores preference. Does not hot-swap the live pool.
        </div>
      </div>
    {/if}

    <div style="margin-bottom:8px;display:flex;align-items:center;gap:12px;flex-wrap:wrap">
      <button onclick={confirmConnection} disabled={testing}>
        {testing ? "Checking…" : configured ? "Re-check connection" : "Confirm & continue"}
      </button>
      {#if testResult}
        <span style="color:var(--success);font-size:13px">{testResult}</span>
      {/if}
    </div>
    {#if testError}
      <div class="error-box">{testError}</div>
    {/if}
  {/if}

  <div class="wizard-actions">
    <button onclick={() => push("/setup")}>Back</button>
    <button class="primary" onclick={() => push("/setup/llm")} disabled={!configured && !info?.connected}>
      Continue
    </button>
  </div>
</WizardShell>

<style>
  .db-status {
    border: 1px solid var(--border, #333);
    border-radius: 8px;
    padding: 12px 14px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-bottom: 8px;
  }
  .db-status-row {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    font-size: 13px;
    align-items: baseline;
  }
  .db-status-row span {
    color: var(--text-muted);
  }
  .linkish {
    background: none;
    border: none;
    padding: 0;
    color: var(--accent-soft, var(--accent));
    cursor: pointer;
    font-size: 13px;
    text-decoration: underline;
    text-underline-offset: 2px;
  }
</style>
