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
  let filterSeverity = $state("");
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
      if (filterRepo) params.set("repo", filterRepo);
      if (filterStatus) params.set("status", filterStatus);
      if (filterSeverity) params.set("severity", filterSeverity);
      params.set("page", String(page));
      params.set("per_page", "20");

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
            <option value="passing">Passing</option>
            <option value="failing">Failing</option>
            <option value="pending">Pending</option>
            <option value="in_progress">In Progress</option>
          </select>
        </div>
        <div class="form-group">
          <label for="filter-severity">Severity</label>
          <select id="filter-severity" bind:value={filterSeverity} onchange={handleFilterChange}>
            <option value="">All</option>
            <option value="blocking">Blocking</option>
            <option value="warning">Warning</option>
            <option value="info">Info</option>
          </select>
        </div>
      </div>

      <LoadingSpinner loading={loading} />

      {#if error}
        <ErrorState message={error} />
      {:else if loading}
        <!-- spinner shown -->
      {:else if reviews.length === 0}
        <EmptyState message="No reviews found" />
      {:else}
        {#each reviews as review}
          <div class="review-card" style="margin-bottom:8px" onclick={() => goToReview(review.id)}>
            <h3>{review.pr_title ?? `PR #${review.pr_number}`}</h3>
            <div class="review-meta">
              <span>{review.repo_name ?? ""}</span>
              <span class="status-badge {review.status}">{review.status}</span>
              <span>{review.created_at ? new Date(review.created_at).toLocaleString() : ""}</span>
            </div>
          </div>
        {/each}

        <Pagination {page} {totalPages} onChange={(p) => { page = p; loadReviews(); }} />
      {/if}
    </div>
  </div>
</div>
