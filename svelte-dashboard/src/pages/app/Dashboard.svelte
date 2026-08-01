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

  function acceptTone(rate) {
    if (rate == null) return "";
    if (rate >= 90) return "success";
    if (rate >= 70) return "warning";
    return "danger";
  }

  function fpTone(ratio) {
    if (ratio == null) return "";
    if (ratio <= 0.05) return "success";
    if (ratio <= 0.15) return "warning";
    return "danger";
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
          <p class="page-description">Review throughput, trust signals, and recent PR agent activity.</p>
        </div>
      </div>

      <LoadingSpinner loading={loading} />

      {#if error}
        <ErrorState message={error} />
      {:else if loading}
        <div class="stats-skeleton" aria-hidden="true">
          {#each Array(4) as _}
            <div class="skel-card skeleton"></div>
          {/each}
        </div>
        <div class="stats-skeleton" aria-hidden="true">
          {#each Array(4) as _}
            <div class="skel-card skeleton"></div>
          {/each}
        </div>
      {:else if stats}
        <div class="stats-row">
          <StatsCard label="Repos Monitored" value={stats.total_repos ?? 0} />
          <StatsCard label="Reviews Today" value={stats.total_reviews_today ?? 0} />
          <StatsCard
            label="Pass Rate"
            value={stats.pass_rate != null ? `${Math.round(stats.pass_rate)}%` : "—"}
            tone={stats.pass_rate != null && stats.pass_rate >= 80 ? "success" : ""}
          />
          <StatsCard label="Active Findings" value={stats.total_findings ?? 0} />
        </div>

        <section class="section-block trust-panel">
          <div class="page-toolbar compact" style="margin-bottom: var(--space-4)">
            <div>
              <p class="eyebrow">Trust</p>
              <h2 class="page-title" style="font-size: var(--text-xl); margin: 0">Finding quality</h2>
            </div>
          </div>
          <div class="stats-row">
            <StatsCard
              label="Accept rate"
              value={stats.trust?.accept_rate != null
                ? `${Math.round(stats.trust.accept_rate)}%`
                : "—"}
              hint="Findings not dismissed"
              tone={acceptTone(stats.trust?.accept_rate)}
            />
            <StatsCard
              label="FP proxy"
              value={stats.trust?.fp_proxy_ratio != null
                ? Number(stats.trust.fp_proxy_ratio).toFixed(2)
                : "—"}
              hint="Dismissals ÷ Tier-1"
              tone={fpTone(stats.trust?.fp_proxy_ratio)}
            />
            <StatsCard label="Tier-1 findings" value={stats.trust?.tier1_findings ?? 0} tone="info" />
            <StatsCard label="Dismissals" value={stats.trust?.dismissals ?? 0} />
          </div>
        </section>

        <section class="section-block">
          <div class="page-toolbar compact">
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
            <div class="activity-list">
              {#each recentReviews as review, i}
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
                    <span>{review.created_at ? new Date(review.created_at).toLocaleString() : ""}</span>
                  </div>
                </div>
              {/each}
            </div>
          {/if}
        </section>
      {:else}
        <EmptyState message="No data available" />
      {/if}
    </div>
  </div>
</div>
