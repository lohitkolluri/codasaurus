<script>
  import { onMount } from "svelte";
  import { push } from "svelte-spa-router";
  import { api } from "../../stores/api.js";
  import AppShell from "../../lib/AppShell.svelte";
  import LoadingSpinner from "../../lib/LoadingSpinner.svelte";
  import EmptyState from "../../lib/EmptyState.svelte";
  import ErrorState from "../../lib/ErrorState.svelte";
  import Pagination from "../../lib/Pagination.svelte";

  let reviews = $state([]);
  let loading = $state(true);
  let error = $state("");

  let filterRepo = $state("");
  let filterStatus = $state("");
  let repos = $state([]);

  let page = $state(1);
  let totalPages = $state(1);

  onMount(async () => {
    try {
      repos = await api.get("/api/repos");
    } catch {
      // non-critical
    }
    await loadReviews();
  });

  async function loadReviews() {
    loading = true;
    error = "";
    try {
      const params = new URLSearchParams();
      if (filterRepo) params.set("repo_id", filterRepo);
      if (filterStatus) params.set("status", filterStatus);
      params.set("limit", "20");
      params.set("offset", String((page - 1) * 20));

      const data = await api.get(`/api/reviews?${params.toString()}`);
      reviews = data.reviews ?? data ?? [];
      totalPages = data.total_pages ?? 1;
    } catch (err) {
      error = err.message || "Failed to load reviews";
    } finally {
      loading = false;
    }
  }

  function handleFilterChange() {
    page = 1;
    loadReviews();
  }

  function goToReview(id) {
    push(`/app/reviews/${id}`);
  }

  function formatWhen(iso) {
    if (!iso) return "";
    try {
      return new Date(iso).toLocaleString(undefined, {
        month: "short",
        day: "numeric",
        hour: "2-digit",
        minute: "2-digit",
      });
    } catch {
      return "";
    }
  }
</script>

<AppShell title="Reviews">
  <div class="page-panel reviews-page">
    <div class="page-panel-toolbar reviews-toolbar">
      <div class="reviews-heading">
        <h1 class="page-title">Reviews</h1>
        <p class="page-description">Every automated review and its findings.</p>
      </div>
      <div class="filter-bar reviews-filters">
        <div class="form-group">
          <label for="filter-repo">Repository</label>
          <select id="filter-repo" bind:value={filterRepo} onchange={handleFilterChange}>
            <option value="">All</option>
            {#each repos as r}
              <option value={r.id}>{r.full_name ?? r.name}</option>
            {/each}
          </select>
        </div>
        <div class="form-group">
          <label for="filter-status">Status</label>
          <select id="filter-status" bind:value={filterStatus} onchange={handleFilterChange}>
            <option value="">All</option>
            <option value="passed">Passed</option>
            <option value="failed">Failed</option>
            <option value="pending">Pending</option>
          </select>
        </div>
      </div>
    </div>

    <LoadingSpinner loading={loading} />

    {#if error}
      <ErrorState message={error} />
    {:else if loading}
      <div class="stats-skeleton" aria-hidden="true">
        {#each Array(3) as _}
          <div class="skel-card skeleton" style="min-height:72px"></div>
        {/each}
      </div>
    {:else if reviews.length === 0}
      <EmptyState
        message="No reviews found"
        actionLabel="Open dashboard"
        onAction={() => push("/app/dashboard")}
      />
    {:else}
      <div class="page-panel-scroll reviews-scroll">
        <div class="activity-list">
          {#each reviews as review, i}
            <div
              class="review-card"
              style="--stagger: {Math.min(i, 8)}"
              onclick={() => goToReview(review.id)}
              role="button"
              tabindex="0"
              onkeydown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  goToReview(review.id);
                }
              }}
            >
              <div class="review-card-top">
                <h3>{review.pr_title ?? `PR #${review.pr_number}`}</h3>
                <span class="status-badge {review.status}">{review.status}</span>
              </div>
              <div class="review-meta">
                <span class="review-repo">{review.repo_name ?? `Repo #${review.repo_id}`}</span>
                <span class="review-sep" aria-hidden="true">·</span>
                <span class="review-when">{formatWhen(review.created_at)}</span>
              </div>
            </div>
          {/each}
        </div>
      </div>

      <div class="page-panel-footer">
        <Pagination
          {page}
          {totalPages}
          onChange={(p) => {
            page = p;
            loadReviews();
          }}
        />
      </div>
    {/if}
  </div>
</AppShell>

<style>
  .reviews-toolbar {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    justify-content: space-between;
    gap: var(--space-4);
  }

  .reviews-heading {
    min-width: min(100%, 240px);
    flex: 1 1 240px;
  }

  .reviews-filters {
    margin: 0;
    flex: 0 1 auto;
  }

  .reviews-scroll {
    border: none;
    background: transparent;
    padding: 0;
  }

  .reviews-scroll .activity-list {
    margin: 0;
  }

  .reviews-page :global(.review-card-top) {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-3);
    margin-bottom: var(--space-2);
  }

  .reviews-page :global(.review-card-top h3) {
    margin: 0;
    flex: 1 1 auto;
    min-width: 0;
    line-height: 1.35;
  }

  .reviews-page :global(.review-card-top .status-badge) {
    margin-left: 0;
    flex-shrink: 0;
    text-transform: capitalize;
  }

  .reviews-page :global(.review-meta) {
    gap: var(--space-2);
  }

  .review-sep {
    opacity: 0.45;
  }

  .review-repo {
    font-family: var(--font-mono, ui-monospace, monospace);
    font-size: 12px;
  }

  .review-when {
    color: var(--text-muted);
  }

  @media (max-width: 640px) {
    .reviews-toolbar {
      align-items: stretch;
    }

    .reviews-filters {
      width: 100%;
    }

    .reviews-filters :global(.form-group) {
      flex: 1 1 140px;
    }
  }
</style>
