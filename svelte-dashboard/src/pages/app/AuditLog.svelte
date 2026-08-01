<script>
  import { onMount } from "svelte";
  import { api } from "../../stores/api.js";
  import Sidebar from "../../lib/Sidebar.svelte";
  import Header from "../../lib/Header.svelte";
  import LoadingSpinner from "../../lib/LoadingSpinner.svelte";
  import EmptyState from "../../lib/EmptyState.svelte";
  import ErrorState from "../../lib/ErrorState.svelte";
  import Pagination from "../../lib/Pagination.svelte";

  let entries = $state([]);
  let loading = $state(true);
  let error = $state("");
  let filterEvent = $state("");

  let page = $state(1);
  let totalPages = $state(1);

  onMount(() => loadEntries());

  async function loadEntries() {
    loading = true;
    error = "";
    try {
      const params = new URLSearchParams();
      if (filterEvent) params.set("event_type", filterEvent);
      params.set("page", String(page));
      params.set("per_page", "30");

      const data = await api.get(`/api/audit?${params.toString()}`);
      entries = data.entries ?? data ?? [];
      totalPages = data.total_pages ?? 1;
    } catch (err) {
      error = err.message || "Failed to load audit log";
    } finally {
      loading = false;
    }
  }

  function handleFilterChange() {
    page = 1;
    loadEntries();
  }
</script>

<div class="app-layout">
  <Sidebar />
  <div class="app-main">
    <Header title="Audit Log" />
    <div class="app-content">
      <div class="filter-bar">
        <div class="form-group">
          <label for="filter-event">Event Type</label>
          <select id="filter-event" bind:value={filterEvent} onchange={handleFilterChange}>
            <option value="">All</option>
            <option value="review.passed">Review passed</option>
            <option value="review.failed">Review failed</option>
            <option value="installation.created">Installation created</option>
            <option value="installation.deleted">Installation deleted</option>
            <option value="settings.updated">Settings updated</option>
            <option value="user.login">User login</option>
            <option value="github.config_cleared">GitHub config cleared</option>
          </select>
        </div>
      </div>

      <LoadingSpinner loading={loading} />

      {#if error}
        <ErrorState message={error} />
      {:else if loading}
        <!-- spinner -->
      {:else if entries.length === 0}
        <EmptyState message="No audit log entries" />
      {:else}
        <table>
          <thead>
            <tr>
              <th>Timestamp</th>
              <th>Event</th>
              <th>Actor</th>
              <th>Target</th>
            </tr>
          </thead>
          <tbody>
            {#each entries as entry}
              <tr>
                <td style="white-space:nowrap;font-size:13px">
                  {entry.created_at || entry.timestamp
                    ? new Date(entry.created_at ?? entry.timestamp).toLocaleString()
                    : "-"}
                </td>
                <td style="font-family:var(--font-code);font-size:12px">{entry.event_type ?? entry.event ?? "-"}</td>
                <td>{entry.actor ?? entry.user ?? "-"}</td>
                <td>{entry.target_type ?? entry.target_id ?? "-"}</td>
              </tr>
            {/each}
          </tbody>
        </table>

        <Pagination {page} {totalPages} onChange={(p) => { page = p; loadEntries(); }} />
      {/if}
    </div>
  </div>
</div>
