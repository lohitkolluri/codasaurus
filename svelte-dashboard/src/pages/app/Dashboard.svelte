<script>
  import { onMount } from "svelte";
  import { push } from "svelte-spa-router";
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

  function goToReview(id) {
    if (id != null) push(`/app/reviews/${id}`);
  }

  function onCardKey(e, id) {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      goToReview(id);
    }
  }
</script>

<div class="app-layout">
  <Sidebar />
  <div class="app-main">
    <Header title="Dashboard" />
    <div class="app-content">
      <div class="page-toolbar compact">
        <div>
          <p class="eyebrow">Overview</p>
          <h1 class="page-title">Dashboard</h1>
          <p class="page-description">Review throughput and recent PR agent activity.</p>
        </div>
      </div>

      <LoadingSpinner loading={loading} />

      {#if error}
        <ErrorState message={error} />
      {:else if loading}
        <!-- spinner shown -->
      {:else if stats}
        <div class="stats-row">
          <StatsCard label="Repos Monitored" value={stats.total_repos ?? 0} />
          <StatsCard label="Reviews Today" value={stats.total_reviews_today ?? 0} />
          <StatsCard
            label="Pass Rate"
            value={stats.pass_rate != null ? `${Math.round(stats.pass_rate)}%` : "—"}
          />
          <StatsCard label="Active Findings" value={stats.total_findings ?? 0} />
        </div>

        <div class="page-toolbar compact" style="margin-top: var(--space-6)">
          <div>
            <p class="eyebrow">Feed</p>
            <h2 class="page-title" style="font-size: var(--text-xl)">Recent activity</h2>
          </div>
        </div>

        {#if recentReviews.length === 0}
          <EmptyState
            message="No reviews yet — open a PR on a connected repository."
            actionLabel="View repositories"
            onAction={() => push("/app/repos")}
          />
        {:else}
          {#each recentReviews as review}
            <div
              class="review-card"
              style="margin-bottom:8px;cursor:pointer"
              role="button"
              tabindex="0"
              onclick={() => goToReview(review.id)}
              onkeydown={(e) => onCardKey(e, review.id)}
            >
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
