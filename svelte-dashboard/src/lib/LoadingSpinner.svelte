<script>
  let { loading = false } = $props();
</script>

{#if loading}
  <div class="loading-bar"></div>
{/if}
