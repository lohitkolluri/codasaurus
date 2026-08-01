<script>
  let { label = "", value = "" } = $props();
</script>

<div class="card stats-card">
  <div class="stats-label">{label}</div>
  <div class="stats-value">{value}</div>
</div>

<style>
  .stats-card {
    text-align: left;
    padding: var(--space-5);
  }

  .stats-label {
    font-size: var(--text-xs);
    font-weight: var(--weight-medium);
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--text-muted);
    margin-bottom: var(--space-2);
  }

  .stats-value {
    font-size: var(--text-3xl);
    font-weight: var(--weight-bold);
    line-height: 1.1;
    color: var(--text-primary);
  }
</style>
