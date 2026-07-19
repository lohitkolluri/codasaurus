<script>
  let { page = 1, totalPages = 1, onChange } = $props();

  function goTo(p) {
    if (p >= 1 && p <= totalPages && onChange) {
      onChange(p);
    }
  }

  const visiblePages = $derived.by(() => {
    const pages = [];
    const start = Math.max(1, page - 2);
    const end = Math.min(totalPages, page + 2);
    for (let i = start; i <= end; i++) {
      pages.push(i);
    }
    return pages;
  });
</script>

{#if totalPages > 1}
  <div class="pagination">
    <button disabled={page <= 1} onclick={() => goTo(page - 1)}>Prev</button>

    {#each visiblePages as p}
      <button class="page-num" class:active={p === page} onclick={() => goTo(p)}>
        {p}
      </button>
    {/each}

    <button disabled={page >= totalPages} onclick={() => goTo(page + 1)}>Next</button>
  </div>
{/if}
