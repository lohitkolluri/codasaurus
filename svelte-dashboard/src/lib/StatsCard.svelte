<script>
  let { label = "", value = "", hint = "", tone = "" } = $props();
</script>

<div class="card stats-card" class:tone-success={tone === "success"} class:tone-warning={tone === "warning"} class:tone-danger={tone === "danger"} class:tone-info={tone === "info"}>
  <div class="stats-label">{label}</div>
  <div class="stats-value">{value}</div>
  {#if hint}
    <div class="stats-hint">{hint}</div>
  {/if}
</div>
