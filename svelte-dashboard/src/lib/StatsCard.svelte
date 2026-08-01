<script>
  let {
    label = "",
    value = "",
    hint = "",
    tone = "",
    delta = null,
    deltaLabel = "vs prior 7d",
    spark = null,
  } = $props();

  let deltaGood = $derived.by(() => {
    if (delta == null || Number.isNaN(delta)) return null;
    // tone-aware: for "danger-up" metrics caller passes inverted tone via deltaSign
    return delta;
  });

  let sparkPoints = $derived.by(() => {
    if (!Array.isArray(spark) || spark.length < 2) return "";
    const vals = spark.map((n) => Number(n) || 0);
    const max = Math.max(...vals, 1);
    const w = 64;
    const h = 20;
    return vals
      .map((v, i) => {
        const x = (i / (vals.length - 1)) * w;
        const y = h - (v / max) * (h - 2) - 1;
        return `${x.toFixed(1)},${y.toFixed(1)}`;
      })
      .join(" ");
  });

  function formatDelta(d) {
    if (d == null || Number.isNaN(d)) return null;
    const abs = Math.abs(d);
    const rounded = abs >= 10 ? Math.round(abs) : Math.round(abs * 10) / 10;
    const sign = d > 0 ? "+" : d < 0 ? "−" : "";
    return `${sign}${rounded}`;
  }
</script>

<div
  class="card static stats-card"
  class:tone-success={tone === "success"}
  class:tone-warning={tone === "warning"}
  class:tone-danger={tone === "danger"}
  class:tone-info={tone === "info"}
>
  <div class="stats-card-top">
    <div class="stats-label">{label}</div>
    {#if sparkPoints}
      <svg class="stats-spark" viewBox="0 0 64 20" aria-hidden="true">
        <polyline fill="none" stroke="currentColor" stroke-width="1.5" points={sparkPoints} />
      </svg>
    {/if}
  </div>
  <div class="stats-value">{value}</div>
  <div class="stats-card-meta">
    {#if formatDelta(deltaGood) != null}
      <span
        class="stats-delta"
        class:up={delta > 0}
        class:down={delta < 0}
        class:flat={delta === 0}
      >
        {formatDelta(deltaGood)}
      </span>
      <span class="stats-delta-label">{deltaLabel}</span>
    {:else if hint}
      <span class="stats-hint">{hint}</span>
    {/if}
  </div>
  {#if hint && formatDelta(deltaGood) != null}
    <div class="stats-hint">{hint}</div>
  {/if}
</div>
