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

  onMount(async () => {
    try {
      repos = await api.get("/api/repos");
    } catch (err) {
      error = err.message || "Failed to load repos";
    } finally {
      loading = false;
    }
  });

  function openRepo(id) {
    push(`/app/repos/${id}`);
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
          <button class="primary" onclick={installRepo}>Install repository</button>
        </div>
      </div>

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
            {#each repos as repo}
              <tr style="cursor:pointer" onclick={() => openRepo(repo.id)}>
                <td style="font-weight:600">{repo.full_name ?? repo.name ?? repo.id}</td>
                <td>{repo.default_branch ?? "main"}</td>
                <td>{repo.last_review_at ? new Date(repo.last_review_at).toLocaleDateString() : "—"}</td>
                <td>
                  {#if repo.enabled}
                    <span class="status-badge passing">Active</span>
                  {:else}
                    <span class="status-badge pending">Inactive</span>
                  {/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    </div>
  </div>
</div>
