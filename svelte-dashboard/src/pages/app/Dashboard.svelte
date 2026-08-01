<script>
  import { onMount } from "svelte";
  import { link, push } from "svelte-spa-router";
  import { api } from "../../stores/api.js";
  import AppShell from "../../lib/AppShell.svelte";
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
      error = err.message || "Failed to load dashboard";
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

<AppShell title="Dashboard">
  <div class="dashboard-hero">
    <div>
      <p class="eyebrow">Overview</p>
      <h1 class="page-title">Dashboard</h1>
      <p class="page-description">
        What needs attention right now. Deep charts live on Stats.
      </p>
    </div>
    <div class="dashboard-hero-actions">
      <a class="primary" href="#/app/stats" use:link>Open stats</a>
      <a href="#/app/repos" use:link>Repositories</a>
    </div>
  </div>

  <LoadingSpinner loading={loading} />

  {#if error}
    <ErrorState message={error} />
  {:else if loading}
    <div class="dashboard-kpis" aria-hidden="true">
      {#each Array(4) as _}
        <div class="skel-card skeleton"></div>
      {/each}
    </div>
  {:else if stats}
    <div class="dashboard-kpis">
      <StatsCard label="Repos monitored" value={stats.total_repos ?? 0} />
      <StatsCard label="Reviews today" value={stats.total_reviews_today ?? 0} />
      <StatsCard
        label="Pass rate"
        value={stats.pass_rate != null ? `${Math.round(stats.pass_rate)}%` : "—"}
        tone={stats.pass_rate != null && stats.pass_rate >= 80 ? "success" : ""}
      />
      <StatsCard label="Active findings" value={stats.total_findings ?? 0} />
    </div>

    <section class="dashboard-activity">
      <h2>Recent activity</h2>
      {#if recentReviews.length === 0}
        <EmptyState
          message="No reviews yet. Open a PR on an enabled repository."
          actionLabel="View repositories"
          onAction={() => push("/app/repos")}
        />
      {:else}
        <div class="activity-list">
          {#each recentReviews.slice(0, 8) as review, i}
            <div
              class="review-card"
              style="--stagger: {Math.min(i, 8)}"
              role="button"
              tabindex="0"
              onclick={() => goToReview(review.id)}
              onkeydown={(e) => onCardKey(e, review.id)}
            >
              <h3>{review.pr_title ?? `PR #${review.pr_number ?? review.id}`}</h3>
              <div class="review-meta">
                <span>{review.repo ?? ""}</span>
                <span class="status-badge {review.status}">{review.status}</span>
                <span
                  >{review.created_at
                    ? new Date(review.created_at).toLocaleString()
                    : ""}</span
                >
              </div>
            </div>
          {/each}
        </div>
      {/if}
    </section>
  {:else}
    <EmptyState message="No data available" />
  {/if}
</AppShell>
