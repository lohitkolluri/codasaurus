<script>
  import { onMount } from "svelte";
  import { Chart, DoughnutController, ArcElement, Tooltip, Legend } from "chart.js";

  Chart.register(DoughnutController, ArcElement, Tooltip, Legend);

  let {
    passed = 0,
    failed = 0,
    other = 0,
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
    const success = cssVar("--success", "#3d9a6a");
    const error = cssVar("--error", "#d64545");
    const muted = cssVar("--text-muted", "#888");

    const labels = ["Passed", "Failed"];
    const data = [passed, failed];
    const colors = [success, error];
    if (other > 0) {
      labels.push("Other");
      data.push(other);
      colors.push(muted);
    }

    chart?.destroy();
    chart = new Chart(canvas, {
      type: "doughnut",
      data: {
        labels,
        datasets: [
          {
            data,
            backgroundColor: colors,
            borderWidth: 0,
            hoverOffset: 4,
          },
        ],
      },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        cutout: "68%",
        plugins: {
          legend: {
            position: "bottom",
            labels: {
              boxWidth: 10,
              boxHeight: 10,
              color: textMuted,
              font: { size: 12 },
              padding: 14,
              usePointStyle: true,
              pointStyle: "circle",
            },
          },
          tooltip: {
            backgroundColor: cssVar("--bg-tertiary", "#1a1a1a"),
            titleColor: cssVar("--text-primary", "#fff"),
            bodyColor: textMuted,
            borderColor: border,
            borderWidth: 1,
            padding: 10,
            callbacks: {
              label(ctx) {
                const total = ctx.dataset.data.reduce((a, b) => a + b, 0) || 1;
                const pct = Math.round((ctx.parsed / total) * 100);
                return ` ${ctx.label}: ${ctx.parsed} (${pct}%)`;
              },
            },
          },
        },
      },
    });
  }

  onMount(() => {
    build();
    return () => chart?.destroy();
  });

  $effect(() => {
    passed;
    failed;
    other;
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
    <p>No pass/fail outcomes this week yet.</p>
    {#if action}{@render action()}{/if}
  </div>
{:else}
  <div class="chart-lib-wrap">
    <canvas bind:this={canvas} aria-label="Pass fail outcomes"></canvas>
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
