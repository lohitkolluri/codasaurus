<script>
  import { onMount } from "svelte";
  import { api } from "../../stores/api.js";
  import AppShell from "../../lib/AppShell.svelte";
  import LoadingSpinner from "../../lib/LoadingSpinner.svelte";
  import EmptyState from "../../lib/EmptyState.svelte";
  import ErrorState from "../../lib/ErrorState.svelte";
  import Pagination from "../../lib/Pagination.svelte";

  const DEFAULT_RETENTION_DAYS = 90;

  let entries = $state([]);
  let loading = $state(true);
  let error = $state("");
  let filterEvent = $state("");
  let retentionDays = $state(DEFAULT_RETENTION_DAYS);

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
      if (typeof data.retention_days === "number" && data.retention_days > 0) {
        retentionDays = data.retention_days;
      }
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

<AppShell title="Audit log">
  <div class="page-panel">
    <div class="page-panel-toolbar">
      <div class="page-toolbar compact" style="margin-bottom: 0">
        <div>
          <p class="eyebrow">Security</p>
          <h1 class="page-title">Audit log</h1>
          <p class="page-description">
            Security and configuration events for this instance.
          </p>
        </div>
      </div>
      <div class="filter-bar" style="margin-top: var(--space-4); margin-bottom: 0">
        <div class="form-group">
          <label for="filter-event">Event type</label>
          <select id="filter-event" bind:value={filterEvent} onchange={handleFilterChange}>
            <option value="">All</option>
            <option value="review.passed">Review passed</option>
            <option value="review.failed">Review failed</option>
            <option value="installation.created">Installation created</option>
            <option value="installation.deleted">Installation deleted</option>
            <option value="settings.updated">Settings updated</option>
            <option value="user.login">User login</option>
            <option value="user.invite">Invite created</option>
            <option value="user.invite_revoke">Invite revoked</option>
            <option value="user.role_change">Role changed</option>
            <option value="user.remove">Member removed</option>
            <option value="user.bootstrap_transfer">Bootstrap transfer</option>
            <option value="user.accept">Invite accepted</option>
            <option value="github.config_cleared">GitHub config cleared</option>
          </select>
        </div>
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
      <div class="page-panel-scroll">
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
                    : "—"}
                </td>
                <td style="font-family:var(--font-code);font-size:12px"
                  >{entry.event_type ?? entry.event ?? "—"}</td
                >
                <td>{entry.actor ?? entry.user ?? "—"}</td>
                <td>{entry.target_type ?? entry.target_id ?? "—"}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>

      <div class="page-panel-footer">
        <p class="page-panel-note">
          Entries older than {retentionDays} days are deleted automatically.
        </p>
        <Pagination
          {page}
          {totalPages}
          onChange={(p) => {
            page = p;
            loadEntries();
          }}
        />
      </div>
    {/if}
  </div>
</AppShell>
