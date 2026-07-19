<script>
  import { onMount } from "svelte";
  import { api } from "../../stores/api.js";
  import Sidebar from "../../lib/Sidebar.svelte";
  import Header from "../../lib/Header.svelte";
  import StatsCard from "../../lib/StatsCard.svelte";
  import LoadingSpinner from "../../lib/LoadingSpinner.svelte";
  import EmptyState from "../../lib/EmptyState.svelte";
  import ErrorState from "../../lib/ErrorState.svelte";

  let stats = $state(null);
  let recentReviews = $state([]);
  let loading = $state(true);
  let error = $state("");

  onMount(async () => {
    try {
      const data = await api.get("/api/stats");
      stats = data;
      recentReviews = data.recent_activity ?? [];
    } catch (err) {
      error = err.message || "Failed to load stats";
    } finally {
      loading = false;
    }
  });
</script>

<div class="app-layout">
  <Sidebar />
  <div class="app-main">
    <Header title="Dashboard" />
    <div class="app-content">
      <LoadingSpinner loading={loading} />

      {#if error}
        <ErrorState message={error} />
      {:else if loading}
        <!-- loading, spinner shown -->
      {:else if stats}
        <div class="stats-row">
          <StatsCard label="Repos Monitored" value={stats.total_repos ?? 0} />
          <StatsCard label="Reviews Today" value={stats.total_reviews_today ?? 0} />
          <StatsCard label="Pass Rate" value={stats.pass_rate != null ? `${stats.pass_rate}%` : "—"} />
          <StatsCard label="Active Findings" value={stats.total_findings ?? 0} />
        </div>

        <h2 class="page-title" style="font-size:18px">Recent Activity</h2>

        {#if recentReviews.length === 0}
          <EmptyState message="No recent reviews" />
        {:else}
          {#each recentReviews as review}
            <div class="review-card" style="margin-bottom:8px">
              <h3>{review.pr_title ?? `PR #${review.pr_number ?? review.id}`}</h3>
              <div class="review-meta">
                <span>{review.repo ?? ""}</span>
                <span class="status-badge {review.status}">{review.status}</span>
                <span>{review.created_at ? new Date(review.created_at).toLocaleString() : ""}</span>
              </div>
            </div>
          {/each}
        {/if}
      {:else}
        <EmptyState message="No data available" />
      {/if}
    </div>
  </div>
</div>
