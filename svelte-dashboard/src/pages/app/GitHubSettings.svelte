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

  let rotating = $state(false);
  let uninstalling = $state(false);
  let confirmUninstall = $state(false);
  let msg = $state("");

  onMount(async () => {
    try {
      config = await api.get("/api/settings/github");
    } catch (err) {
      error = err.message || "Failed to load GitHub settings";
    } finally {
      loading = false;
    }
  });

  async function reinstall() {
    try {
      const data = await api.get("/api/github/install-url");
      if (data.url) window.open(data.url, "_blank");
    } catch (err) {
      msg = err.message || "Failed";
    }
  }

  async function rotateCredentials() {
    rotating = true;
    msg = "";
    try {
      await api.post("/api/settings/github/rotate");
      msg = "Credentials rotated";
    } catch (err) {
      msg = err.message || "Rotation failed";
    } finally {
      rotating = false;
    }
  }

  async function uninstall() {
    uninstalling = true;
    msg = "";
    try {
      await api.delete("/api/settings/github");
      config = null;
      msg = "GitHub App uninstalled";
    } catch (err) {
      msg = err.message || "Uninstall failed";
    } finally {
      uninstalling = false;
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
      {:else if config}
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

          <div style="display:flex;gap:12px;margin-top:20px">
            <button onclick={reinstall}>Reinstall App</button>
            <button onclick={rotateCredentials} disabled={rotating}>
              {rotating ? "Rotating…" : "Rotate Credentials"}
            </button>
          </div>
        </div>

        {#if msg}
          <p style="font-size:13px;color:var(--success);margin-bottom:12px">{msg}</p>
        {/if}

        <div class="danger-zone">
          <h3>Danger Zone</h3>
          <p>Uninstalling will remove the GitHub App configuration and disable all repository integrations.</p>
          {#if !confirmUninstall}
            <button class="danger" onclick={() => (confirmUninstall = true)}>Uninstall</button>
          {:else}
            <p style="font-size:13px;margin-bottom:8px">Are you sure? This cannot be undone.</p>
            <div style="display:flex;gap:8px">
              <button class="danger" onclick={uninstall} disabled={uninstalling}>
                {uninstalling ? "Uninstalling…" : "Confirm Uninstall"}
              </button>
              <button onclick={() => (confirmUninstall = false)}>Cancel</button>
            </div>
          {/if}
        </div>
      {:else}
        <div class="card" style="margin-bottom:24px">
          <p style="color:var(--text-muted)">No GitHub App configured.</p>
          <button style="margin-top:12px" onclick={reinstall}>Install GitHub App</button>
        </div>
      {/if}
    </div>
  </div>
</div>
