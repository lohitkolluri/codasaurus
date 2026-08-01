<script>
  import { onMount } from "svelte";
  import { api } from "../../stores/api.js";
  import Sidebar from "../../lib/Sidebar.svelte";
  import Header from "../../lib/Header.svelte";
  import LoadingSpinner from "../../lib/LoadingSpinner.svelte";
  import ErrorState from "../../lib/ErrorState.svelte";

  let config = $state(null);
  let loading = $state(true);
  let error = $state("");

  let clearing = $state(false);
  let confirmClear = $state(false);
  let msg = $state("");

  onMount(async () => {
    await reload();
  });

  async function reload() {
    loading = true;
    error = "";
    try {
      config = await api.get("/api/settings/github");
    } catch (err) {
      error = err.message || "Failed to load GitHub settings";
    } finally {
      loading = false;
    }
  }

  async function reinstall() {
    try {
      const data = await api.get("/api/github/install-url");
      if (data.url) window.open(data.url, "_blank");
    } catch (err) {
      msg = err.message || "Failed to open install URL";
    }
  }

  async function clearLocalConfig() {
    clearing = true;
    msg = "";
    try {
      const res = await api.delete("/api/settings/github");
      config = { configured: false };
      confirmClear = false;
      msg =
        res?.message ||
        "Local GitHub App config cleared. Repos are marked inactive. Uninstall the App from GitHub separately if you want it gone there too.";
    } catch (err) {
      msg = err.message || "Clear failed";
    } finally {
      clearing = false;
    }
  }
</script>

<div class="app-layout">
  <Sidebar />
  <div class="app-main">
    <Header title="GitHub Settings" />
    <div class="app-content">
      <LoadingSpinner loading={loading} />

      {#if error}
        <ErrorState message={error} />
      {:else if loading}
        <!-- spinner -->
      {:else if config?.configured}
        <div class="card" style="margin-bottom:24px">
          <h3 style="font-size:16px;font-weight:600;margin-bottom:16px">GitHub App</h3>

          <div style="margin-bottom:12px">
            <span style="font-size:13px;color:var(--text-muted)">App Name</span>
            <p style="font-size:14px">{config.app_name ?? "—"}</p>
          </div>
          <div style="margin-bottom:12px">
            <span style="font-size:13px;color:var(--text-muted)">App ID</span>
            <p style="font-size:14px">{config.app_id ?? "—"}</p>
          </div>

          <div style="display:flex;gap:12px;margin-top:20px;flex-wrap:wrap">
            <button onclick={reinstall}>Open install URL</button>
            <a
              href="https://github.com/settings/apps"
              target="_blank"
              rel="noopener noreferrer"
              class="btn-link"
              style="display:inline-flex;align-items:center;font-size:13px"
            >Manage App on GitHub ↗</a>
          </div>
          <p style="font-size:12px;color:var(--text-muted);margin-top:12px">
            To rotate the private key or webhook secret, generate new credentials in GitHub, then clear local config below and re-run setup (or set the env vars and redeploy).
          </p>
        </div>

        {#if msg}
          <p style="font-size:13px;margin-bottom:12px;color:{/fail|Fail|error/i.test(msg) ? 'var(--error)' : 'var(--success)'}">{msg}</p>
        {/if}

        <div class="danger-zone">
          <h3>Danger Zone</h3>
          <p>
            Clears App ID, private key, and webhook secret from Codasaurus and marks synced repos inactive.
            This does <strong>not</strong> uninstall the App from GitHub.
          </p>
          {#if !confirmClear}
            <button class="danger" onclick={() => (confirmClear = true)}>Clear local GitHub config</button>
          {:else}
            <p style="font-size:13px;margin-bottom:8px">Clear local credentials? The GitHub App itself stays installed until you remove it on GitHub.</p>
            <div style="display:flex;gap:8px">
              <button class="danger" onclick={clearLocalConfig} disabled={clearing}>
                {clearing ? "Clearing…" : "Confirm clear"}
              </button>
              <button onclick={() => (confirmClear = false)}>Cancel</button>
            </div>
          {/if}
        </div>
      {:else}
        <div class="card" style="margin-bottom:24px">
          <p style="color:var(--text-muted)">No GitHub App configured in Codasaurus.</p>
          {#if msg}
            <p style="font-size:13px;margin:12px 0;color:var(--success)">{msg}</p>
          {/if}
          <button style="margin-top:12px" onclick={reinstall}>Install GitHub App</button>
        </div>
      {/if}
    </div>
  </div>
</div>
