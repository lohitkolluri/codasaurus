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
  let syncError = $state(false);
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
    syncError = false;
    try {
      const data = await api.post("/api/repos/sync");
      syncMsg = `Synced ${data.synced} repos`;
      repos = await api.get("/api/repos");
    } catch (err) {
      syncMsg = "Sync failed: " + (err.message || "unknown error");
      syncError = true;
    } finally {
      syncing = false;
    }
  }

  function openRepo(id) {
    push(`/app/repos/${id}`);
  }

  async function toggleRepo(id, current) {
    try {
      const repo = repos.find((r) => r.id === id);
      await api.put(`/api/repos/${id}`, {
        config_json: repo?.config_json ?? "{}",
        active: !current,
      });
      repos = await api.get("/api/repos");
    } catch (err) {
      syncMsg = "Toggle failed: " + (err.message || "unknown error");
      syncError = true;
    }
  }

  async function batchToggle(active) {
    syncError = false;
    try {
      // Chunk into batches of 20 to avoid overwhelming the server
      const chunk = (arr, size) => {
        const result = [];
        for (let i = 0; i < arr.length; i += size) result.push(arr.slice(i, i + size));
        return result;
      };
      for (const batch of chunk(filtered, 20)) {
        await Promise.all(
          batch.map((r) =>
            api.put(`/api/repos/${r.id}`, {
              config_json: r.config_json ?? "{}",
              active,
            }),
          ),
        );
      }
      repos = await api.get("/api/repos");
      syncMsg = `${filtered.length} repos ${active ? "enabled" : "disabled"}`;
    } catch (err) {
      syncMsg = "Batch update failed: " + (err.message || "unknown error");
      syncError = true;
    }
  }

  async function installRepo() {
    const popup = window.open("", "_blank");
    try {
      const data = await api.get("/api/github/install-url");
      if (data.url && popup) popup.location.href = data.url;
    } catch (err) {
      error = err.message || "Failed to get install URL";
      popup?.close();
    }
  }

  async function manageRepos() {
    const popup = window.open("", "_blank");
    try {
      const data = await api.get("/api/github/manage-url");
      if (data.url && popup) popup.location.href = data.url;
    } catch {
      if (popup) popup.close();
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
        <p class="inline-flash" class:error={syncError}>{syncMsg}</p>
      {/if}

      {#if repos.length > 0}
        <div class="search-bar">
          <input type="search" placeholder="Search repositories…" bind:value={search} />
          <div class="search-actions">
            <span class="filter-count">{filtered.length} of {repos.length}</span>
            <button onclick={() => batchToggle(true)}>Enable all</button>
            <button onclick={() => batchToggle(false)}>Disable all</button>
          </div>
        </div>
      {/if}

      {#if error}
        <ErrorState message={error} />
      {:else if loading}
        <!-- spinner shown -->
      {:else if repos.length === 0}
        <EmptyState message="No repositories configured. Install the GitHub App to get started." />
      {:else}
        <div class="table-wrapper">
          <table>
            <thead>
              <tr>
                <th>Repository</th>
                <th>Default Branch</th>
                <th>Last Review</th>
                <th style="width:100px">Status</th>
              </tr>
            </thead>
            <tbody>
              {#each filtered as repo}
                <tr onclick={() => openRepo(repo.id)}>
                  <td class="repo-cell">
                    <span class="repo-icon" aria-hidden="true">
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
                      </svg>
                    </span>
                    <span class="repo-name">{repo.full_name}</span>
                  </td>
                  <td><code>{repo.default_branch ?? "main"}</code></td>
                  <td>{repo.updated_at ? new Date(repo.updated_at).toLocaleDateString() : "—"}</td>
                  <td>
                    <button class="toggle-btn" class:active={repo.active} onclick={(e) => { e.stopPropagation(); toggleRepo(repo.id, repo.active); }}>
                      {repo.active ? "Active" : "Inactive"}
                    </button>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  .search-bar {
    display: flex;
    gap: 12px;
    margin-bottom: 16px;
    align-items: center;
    flex-wrap: wrap;
  }
  .search-bar input {
    flex: 1;
    min-width: 200px;
    padding: 9px 14px;
    border: 1px solid var(--border);
    border-radius: 8px;
    font-size: 14px;
    background: var(--bg-primary);
    color: var(--text-primary);
    outline: none;
    transition: border-color 0.15s;
  }
  .search-bar input:focus {
    border-color: var(--accent);
  }
  .search-actions {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .filter-count {
    font-size: 13px;
    color: var(--text-muted);
    margin-right: 4px;
  }
  .search-actions button {
    font-size: 12px;
    padding: 6px 14px;
    border-radius: 6px;
    border: 1px solid var(--border);
    background: var(--bg-secondary);
    color: var(--text-primary);
    cursor: pointer;
  }
  .search-actions button:hover {
    background: var(--accent);
    color: #fff;
    border-color: var(--accent);
  }

  .table-wrapper {
    border: 1px solid var(--border);
    border-radius: 8px;
    overflow: auto;
    max-height: calc(100vh - 360px);
    background: var(--bg-primary);
  }
  table {
    width: 100%;
    border-collapse: collapse;
  }
  thead {
    position: sticky;
    top: 0;
    z-index: 1;
  }
  th {
    background: var(--bg-secondary);
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--text-muted);
    padding: 10px 14px;
    text-align: left;
    border-bottom: 1px solid var(--border);
  }
  td {
    padding: 10px 14px;
    border-bottom: 1px solid var(--border);
    font-size: 14px;
    vertical-align: middle;
  }
  tbody tr:last-child td {
    border-bottom: none;
  }
  tbody tr {
    cursor: pointer;
    transition: background 0.1s;
  }
  tbody tr:hover {
    background: var(--bg-secondary);
  }
  .repo-cell {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .repo-icon {
    display: inline-grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border-radius: 6px;
    border: 1px solid var(--border);
    background: var(--bg-secondary);
    color: var(--accent-soft);
    flex-shrink: 0;
  }
  .inline-flash {
    font-size: 13px;
    margin-bottom: 12px;
    color: var(--success);
    animation: rise-in 120ms var(--ease-out) both;
  }
  .inline-flash.error { color: var(--error); }
  .repo-name {
    font-weight: 600;
  }
  td code {
    font-size: 12px;
    background: var(--bg-secondary);
    padding: 2px 8px;
    border-radius: 4px;
  }
  .toggle-btn {
    font-size: 12px;
    padding: 4px 12px;
    border-radius: 5px;
    border: 1px solid var(--border);
    background: var(--bg-secondary);
    color: var(--text-muted);
    cursor: pointer;
    font-weight: 500;
    transition: all 0.12s;
    width: 100%;
  }
  .toggle-btn.active {
    background: var(--success);
    color: #fff;
    border-color: var(--success);
  }
</style>
