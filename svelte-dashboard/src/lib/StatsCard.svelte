<script>
  let { label = "", value = "" } = $props();
</script>

<div class="card stats-card">
  <div class="stats-value">{value}</div>
  <div class="stats-label">{label}</div>
</div>

<style>
  .stats-card {
    text-align: center;
  }

  .stats-value {
    font-size: 36px;
    font-weight: 700;
    line-height: 1.1;
    margin-bottom: 4px;
  }

  .stats-label {
    font-size: 13px;
    color: var(--text-muted);
  }
</style>
