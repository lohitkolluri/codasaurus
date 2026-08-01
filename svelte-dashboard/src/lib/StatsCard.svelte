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

  let sparkGeom = $derived.by(() => {
    if (!Array.isArray(spark) || spark.length < 2) return null;
    const vals = spark.map((n) => Number(n) || 0);
    const max = Math.max(...vals, 0);
    if (max <= 0) return null;
    const w = 72;
    const h = 28;
    const pts = vals.map((v, i) => {
      const x = (i / (vals.length - 1)) * w;
      const y = h - (v / max) * (h - 4) - 2;
      return { x, y };
    });
    const line = pts.map((p, i) => `${i === 0 ? "M" : "L"}${p.x.toFixed(1)},${p.y.toFixed(1)}`).join(" ");
    const area =
      line +
      ` L${pts[pts.length - 1].x.toFixed(1)},${h}` +
      ` L${pts[0].x.toFixed(1)},${h} Z`;
    return { w, h, line, area };
  });

  function formatDelta(d) {
    if (d == null || Number.isNaN(d)) return null;
    const abs = Math.abs(d);
    const rounded = abs >= 10 ? Math.round(abs) : Math.round(abs * 10) / 10;
    const sign = d > 0 ? "+" : d < 0 ? "−" : "";
    return `${sign}${rounded}`;
  }

  let deltaText = $derived(formatDelta(delta));
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
    {#if sparkGeom}
      <svg class="stats-spark" viewBox={`0 0 ${sparkGeom.w} ${sparkGeom.h}`} aria-hidden="true">
        <path class="stats-spark-area" d={sparkGeom.area} />
        <path class="stats-spark-line" d={sparkGeom.line} fill="none" />
      </svg>
    {/if}
  </div>
  <div class="stats-value">{value}</div>
  <div class="stats-card-meta">
    {#if deltaText != null}
      <span
        class="stats-delta"
        class:up={delta > 0}
        class:down={delta < 0}
        class:flat={delta === 0}
      >
        {deltaText}
      </span>
      <span class="stats-delta-label">{deltaLabel}</span>
    {:else if hint}
      <span class="stats-hint">{hint}</span>
    {/if}
  </div>
  {#if hint && deltaText != null}
    <div class="stats-hint">{hint}</div>
  {/if}
</div>
