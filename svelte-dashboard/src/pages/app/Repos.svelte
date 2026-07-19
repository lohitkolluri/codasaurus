<script>
  import { onMount } from "svelte";
  import { push } from "svelte-spa-router";
  import { api } from "../../stores/api.js";
  import Sidebar from "../../lib/Sidebar.svelte";
  import Header from "../../lib/Header.svelte";
  import LoadingSpinner from "../../lib/LoadingSpinner.svelte";
  import EmptyState from "../../lib/EmptyState.svelte";
  import ErrorState from "../../lib/ErrorState.svelte";

  let repos = $state([]);
  let loading = $state(true);
  let error = $state("");
  let syncing = $state(false);
  let syncMsg = $state("");
  let search = $state("");

  let filtered = $derived.by(() => {
    if (!search) return repos;
    const q = search.toLowerCase();
    return repos.filter(r => (r.full_name ?? "").toLowerCase().includes(q));
  });

  onMount(async () => {
    try {
      repos = await api.get("/api/repos");
    } catch (err) {
      error = err.message || "Failed to load repos";
    } finally {
      loading = false;
    }
  });

  async function syncRepos() {
    syncing = true;
    syncMsg = "";
    try {
      const data = await api.post("/api/repos/sync");
      syncMsg = `Synced ${data.synced} repos`;
      repos = await api.get("/api/repos");
    } catch (err) {
      syncMsg = "Sync failed: " + (err.message || "unknown error");
    } finally {
      syncing = false;
    }
  }

  function openRepo(id) {
    push(`/app/repos/${id}`);
  }

  async function toggleRepo(id, current) {
    try {
      await api.put(`/api/repos/${id}`, { config_json: "", active: !current });
      repos = await api.get("/api/repos");
    } catch (err) {
      syncMsg = "Toggle failed: " + (err.message || "unknown error");
    }
  }

  async function batchToggle(active) {
    try {
      await Promise.all(filtered.map(r => api.put(`/api/repos/${r.id}`, { config_json: "", active })));
      repos = await api.get("/api/repos");
      syncMsg = `${filtered.length} repos ${active ? "enabled" : "disabled"}`;
    } catch (err) {
      syncMsg = "Batch update failed: " + (err.message || "unknown error");
    }
  }

  async function installRepo() {
    try {
      const data = await api.get("/api/github/install-url");
      if (data.url) {
        window.open(data.url, "_blank");
      }
    } catch (err) {
      error = err.message || "Failed to get install URL";
    }
  }

  async function manageRepos() {
    try {
      const data = await api.get("/api/github/manage-url");
      if (data.url) {
        window.open(data.url, "_blank");
      }
    } catch {
      // fallback to install URL
      installRepo();
    }
  }
</script>

<div class="app-layout">
  <Sidebar />
  <div class="app-main">
    <Header title="Repos" />
    <div class="app-content">
      <LoadingSpinner loading={loading} />

      <div class="page-toolbar">
        <div>
          <p class="eyebrow">Workspace</p>
          <h1 class="page-title">Repositories</h1>
          <p class="page-description">Manage the codebases Codasaurus reviews for your team.</p>
        </div>
        <div class="toolbar-actions">
          <span class="toolbar-count">{repos.length} configured</span>
          <button onclick={manageRepos}>Configure repos on GitHub</button>
          <button onclick={syncRepos} disabled={syncing}>{syncing ? "Syncing…" : "Sync Repos"}</button>
          <button class="primary" onclick={installRepo}>Install on new repos</button>
        </div>
      </div>

      {#if syncMsg}
        <p style="font-size:13px;color:var(--text-muted);margin-bottom:8px">{syncMsg}</p>
      {/if}

      {#if repos.length > 0}
        <div style="display:flex;gap:8px;margin-bottom:12px;align-items:center">
          <input type="search" placeholder="Search repos…" bind:value={search}
            style="flex:1;padding:8px 12px;border:1px solid var(--border);border-radius:6px;font-size:14px;background:var(--bg-primary);color:var(--text-primary)" />
          <button onclick={() => batchToggle(true)} style="font-size:12px;padding:6px 12px">Enable {filtered.length}</button>
          <button onclick={() => batchToggle(false)} style="font-size:12px;padding:6px 12px">Disable {filtered.length}</button>
        </div>
      {/if}

      {#if error}
        <ErrorState message={error} />
      {:else if loading}
        <!-- spinner shown -->
      {:else if repos.length === 0}
        <EmptyState message="No repositories configured. Install the GitHub App to get started." />
      {:else}
        <table>
          <thead>
            <tr>
              <th>Repository</th>
              <th>Default Branch</th>
              <th>Last Review</th>
              <th>Status</th>
            </tr>
          </thead>
          <tbody>
            {#each filtered as repo}
              <tr>
                <td style="font-weight:600;cursor:pointer" onclick={() => openRepo(repo.id)} role="button" tabindex="0">{repo.full_name ?? repo.name ?? repo.id}</td>
                <td>{repo.default_branch ?? "main"}</td>
                <td>{repo.updated_at ? new Date(repo.updated_at).toLocaleDateString() : "—"}</td>
                <td>
                  <button class="toggle-repo" class:active={repo.active} onclick={() => toggleRepo(repo.id, repo.active)}>
                    {repo.active ? "Active" : "Inactive"}
                  </button>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    </div>
  </div>
</div>

<style>
  .toggle-repo {
    font-size: 12px;
    padding: 4px 10px;
    border-radius: 4px;
    border: 1px solid var(--border);
    background: var(--bg-secondary);
    color: var(--text-muted);
    cursor: pointer;
  }
  .toggle-repo.active {
    background: var(--accent);
    color: #fff;
    border-color: var(--accent);
  }
</style>
