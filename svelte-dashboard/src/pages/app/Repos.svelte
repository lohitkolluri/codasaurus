<script>
  import { onMount } from "svelte";
  import { push } from "svelte-spa-router";
  import { api } from "../../stores/api.js";
  import { isMaintainer } from "../../stores/auth.js";
  import AppShell from "../../lib/AppShell.svelte";
  import LoadingSpinner from "../../lib/LoadingSpinner.svelte";
  import EmptyState from "../../lib/EmptyState.svelte";
  import ErrorState from "../../lib/ErrorState.svelte";
  import Pagination from "../../lib/Pagination.svelte";

  const PAGE_SIZE = 25;

  let repos = $state([]);
  let loading = $state(true);
  let error = $state("");
  let syncing = $state(false);
  let syncMsg = $state("");
  let syncError = $state(false);
  let search = $state("");
  let page = $state(1);
  let canManage = $derived($isMaintainer);

  let filtered = $derived.by(() => {
    if (!search) return repos;
    const q = search.toLowerCase();
    return repos.filter((r) => (r.full_name ?? "").toLowerCase().includes(q));
  });

  let totalPages = $derived(Math.max(1, Math.ceil(filtered.length / PAGE_SIZE)));
  let pageSafe = $derived(Math.min(Math.max(1, page), totalPages));
  let pageRepos = $derived.by(() => {
    const start = (pageSafe - 1) * PAGE_SIZE;
    return filtered.slice(start, start + PAGE_SIZE);
  });

  let prevSearch = $state("");
  $effect(() => {
    if (prevSearch !== search) {
      page = 1;
      prevSearch = search;
    }
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
      syncMsg = `Synced ${data.synced} repos (new ones stay inactive until you enable them)`;
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

<AppShell title="Repositories">
  <div class="page-panel">
    <div class="page-panel-toolbar">
      <LoadingSpinner loading={loading} />

      <div class="page-toolbar">
        <div>
          <h1 class="page-title">Repositories</h1>
          <p class="page-description">
        Synced repos start inactive. Enable only the ones you want reviewed.
          </p>
        </div>
        <div class="toolbar-actions">
          <span class="toolbar-count">{repos.length} synced</span>
          <button type="button" onclick={manageRepos}>Configure repos on GitHub</button>
          <button type="button" onclick={syncRepos} disabled={syncing || !canManage}
            >{syncing ? "Syncing…" : "Sync Repos"}</button
          >
          <button type="button" class="primary" onclick={installRepo}>Install on new repos</button>
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
            <button type="button" onclick={() => batchToggle(true)} disabled={!canManage}
              >Enable all</button
            >
            <button type="button" onclick={() => batchToggle(false)} disabled={!canManage}
              >Disable all</button
            >
          </div>
        </div>
      {/if}
    </div>

    {#if error}
      <ErrorState message={error} />
    {:else if loading}
      <!-- spinner -->
    {:else if repos.length === 0}
      <EmptyState
        message="No repositories yet. Install the GitHub App, then sync."
        actionLabel="Install on GitHub"
        onAction={installRepo}
      />
      <div class="repos-empty-actions">
        <button type="button" onclick={syncRepos} disabled={syncing || !canManage}>
          {syncing ? "Syncing…" : "Sync repos"}
        </button>
      </div>
    {:else}
      <div class="page-panel-scroll">
        <table>
          <thead>
            <tr>
              <th>Repository</th>
              <th>Default Branch</th>
              <th>Updated</th>
              <th style="width:100px">Status</th>
            </tr>
          </thead>
          <tbody>
            {#each pageRepos as repo}
              <tr onclick={() => openRepo(repo.id)}>
                <td class="repo-cell">
                  <span class="repo-icon" aria-hidden="true">
                    <svg
                      width="14"
                      height="14"
                      viewBox="0 0 24 24"
                      fill="none"
                      stroke="currentColor"
                      stroke-width="1.8"
                      stroke-linecap="round"
                      stroke-linejoin="round"
                    >
                      <path
                        d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"
                      />
                    </svg>
                  </span>
                  <span class="repo-name">{repo.full_name}</span>
                </td>
                <td><code>{repo.default_branch ?? "main"}</code></td>
                <td
                  >{repo.updated_at
                    ? new Date(repo.updated_at).toLocaleDateString()
                    : "n/a"}</td
                >
                <td>
                  <button
                    type="button"
                    class="toggle-btn"
                    class:active={repo.active}
                    disabled={!canManage}
                    onclick={(e) => {
                      e.stopPropagation();
                      if (canManage) toggleRepo(repo.id, repo.active);
                    }}
                  >
                    {repo.active ? "Active" : "Inactive"}
                  </button>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
      {#if filtered.length > PAGE_SIZE}
        <div class="page-panel-footer">
          <span class="filter-count">
            Showing {(pageSafe - 1) * PAGE_SIZE + 1}–{Math.min(pageSafe * PAGE_SIZE, filtered.length)}
            of {filtered.length}
          </span>
          <Pagination
            page={pageSafe}
            {totalPages}
            onChange={(p) => (page = p)}
          />
        </div>
      {/if}
    {/if}
  </div>
</AppShell>

<style>
  .search-bar {
    display: flex;
    gap: 12px;
    margin-top: 12px;
    align-items: center;
    flex-wrap: wrap;
  }
  .repos-empty-actions {
    display: flex;
    justify-content: center;
    margin-top: 12px;
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
  }
  .inline-flash {
    font-size: 13px;
    margin: 12px 0 0;
    color: var(--success);
  }
  .inline-flash.error {
    color: var(--error);
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
  .repo-name {
    font-weight: 600;
  }
  td code {
    font-size: 12px;
    background: var(--bg-secondary);
    padding: 2px 8px;
    border-radius: 4px;
  }
  tbody tr {
    cursor: pointer;
  }
  tbody tr:hover {
    background: var(--bg-secondary);
  }
  th,
  td {
    padding: 10px 14px;
    text-align: left;
    border-bottom: 1px solid var(--border);
    font-size: 14px;
    vertical-align: middle;
  }
  th {
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--text-muted);
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
    width: 100%;
  }
  .toggle-btn.active {
    background: var(--success);
    color: #fff;
    border-color: var(--success);
  }
</style>
