<script>
  import { onMount } from "svelte";
  import { push } from "svelte-spa-router";
  import { api } from "../../stores/api.js";
  import Sidebar from "../../lib/Sidebar.svelte";
  import Header from "../../lib/Header.svelte";
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
</script>

<div class="app-layout">
  <Sidebar />
  <div class="app-main">
    <Header title="Reviews" />
    <div class="app-content">
      <div class="page-toolbar compact">
        <div>
          <p class="eyebrow">Quality signal</p>
          <h1 class="page-title">Reviews</h1>
          <p class="page-description">Inspect every automated review and its findings.</p>
        </div>
      </div>
      <div class="filter-bar">
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
          </select>
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
              <h3>{review.pr_title ?? `PR #${review.pr_number}`}</h3>
              <div class="review-meta">
                <span>{review.repo_name ?? `Repo #${review.repo_id}`}</span>
                <span class="status-badge {review.status}">{review.status}</span>
                <span>{review.created_at ? new Date(review.created_at).toLocaleString() : ""}</span>
              </div>
            </div>
          {/each}
        </div>

        <Pagination {page} {totalPages} onChange={(p) => { page = p; loadReviews(); }} />
      {/if}
    </div>
  </div>
</div>
