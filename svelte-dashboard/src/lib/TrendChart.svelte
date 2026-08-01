<script>
  import { onMount } from "svelte";
  import {
    Chart,
    BarController,
    BarElement,
    CategoryScale,
    LinearScale,
    Tooltip,
    Legend,
    Filler,
  } from "chart.js";

  Chart.register(BarController, BarElement, CategoryScale, LinearScale, Tooltip, Legend, Filler);

  let {
    labels = [],
    reviews = [],
    findings = [],
    empty = false,
    action,
  } = $props();

  let canvas = $state(null);
  let chart;

  function cssVar(name, fallback) {
    const v = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
    return v || fallback;
  }

  function build() {
    if (!canvas || empty) return;
    const textMuted = cssVar("--text-muted", "#888");
    const border = cssVar("--border", "#333");
    const reviewsColor = cssVar("--text-primary", "#f0f0f0");
    const findingsColor = cssVar("--accent-soft", "#e85d4c");

    chart?.destroy();
    chart = new Chart(canvas, {
      type: "bar",
      data: {
        labels,
        datasets: [
          {
            label: "Reviews",
            data: reviews,
            backgroundColor: colorMix(reviewsColor, 0.7),
            borderRadius: 3,
            maxBarThickness: 14,
          },
          {
            label: "Findings",
            data: findings,
            backgroundColor: colorMix(findingsColor, 0.85),
            borderRadius: 3,
            maxBarThickness: 14,
          },
        ],
      },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        interaction: { mode: "index", intersect: false },
        plugins: {
          legend: {
            position: "top",
            align: "end",
            labels: {
              boxWidth: 10,
              boxHeight: 10,
              color: textMuted,
              font: { size: 11 },
              usePointStyle: true,
              pointStyle: "rectRounded",
            },
          },
          tooltip: {
            backgroundColor: cssVar("--bg-tertiary", "#1a1a1a"),
            titleColor: cssVar("--text-primary", "#fff"),
            bodyColor: textMuted,
            borderColor: border,
            borderWidth: 1,
            padding: 10,
          },
        },
        scales: {
          x: {
            grid: { display: false },
            ticks: { color: textMuted, font: { size: 10 }, maxRotation: 0 },
            border: { color: border },
          },
          y: {
            beginAtZero: true,
            ticks: {
              color: textMuted,
              font: { size: 10 },
              precision: 0,
            },
            grid: { color: border },
            border: { display: false },
          },
        },
      },
    });
  }

  function colorMix(hexOrCss, alpha) {
    // chart.js accepts rgba strings; for css vars that are hex, append alpha via canvas trick
    if (hexOrCss.startsWith("#") && hexOrCss.length === 7) {
      const r = parseInt(hexOrCss.slice(1, 3), 16);
      const g = parseInt(hexOrCss.slice(3, 5), 16);
      const b = parseInt(hexOrCss.slice(5, 7), 16);
      return `rgba(${r},${g},${b},${alpha})`;
    }
    return hexOrCss;
  }

  onMount(() => {
    build();
    return () => chart?.destroy();
  });

  $effect(() => {
    labels;
    reviews;
    findings;
    empty;
    if (canvas && !empty) build();
    if (empty) {
      chart?.destroy();
      chart = undefined;
    }
  });
</script>

{#if empty}
  <div class="chart-lib-empty">
    <p>No review activity in the last 14 days.</p>
    {#if action}{@render action()}{/if}
  </div>
{:else}
  <div class="chart-lib-wrap">
    <canvas bind:this={canvas} aria-label="Reviews and findings over 14 days"></canvas>
  </div>
{/if}

<style>
  .chart-lib-wrap {
    position: relative;
    height: 220px;
    width: 100%;
  }

  .chart-lib-empty {
    min-height: 220px;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    justify-content: center;
    gap: 12px;
  }

  .chart-lib-empty p {
    margin: 0;
    font-size: 14px;
    color: var(--text-muted);
  }
</style>
