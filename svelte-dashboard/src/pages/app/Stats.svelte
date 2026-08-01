<script>
  import { onMount } from "svelte";
  import { link } from "svelte-spa-router";
  import { api } from "../../stores/api.js";
  import AppShell from "../../lib/AppShell.svelte";
  import StatsCard from "../../lib/StatsCard.svelte";
  import LoadingSpinner from "../../lib/LoadingSpinner.svelte";
  import EmptyState from "../../lib/EmptyState.svelte";
  import ErrorState from "../../lib/ErrorState.svelte";

  let stats = $state(null);
  let loading = $state(true);
  let error = $state("");

  onMount(async () => {
    try {
      stats = await api.get("/api/stats");
    } catch (err) {
      error = err.message || "Failed to load stats";
    } finally {
      loading = false;
    }
  });

  function acceptTone(rate) {
    if (rate == null) return "";
    if (rate >= 90) return "success";
    if (rate >= 70) return "warning";
    return "danger";
  }

  function fpTone(ratio) {
    if (ratio == null) return "";
    if (ratio <= 0.05) return "success";
    if (ratio <= 0.15) return "warning";
    return "danger";
  }
</script>

<AppShell title="Stats">
  <div class="page-toolbar compact">
    <div>
      <p class="eyebrow">Analytics</p>
      <h1 class="page-title">Stats</h1>
      <p class="page-description">
        Finding quality, weekly rollups, and detector mix — from your Postgres, not a third-party SaaS.
      </p>
    </div>
    <a href="#/app/dashboard" use:link>Back to dashboard</a>
  </div>

  <LoadingSpinner loading={loading} />

  {#if error}
    <ErrorState message={error} />
  {:else if loading}
    <div class="stats-skeleton" aria-hidden="true">
      {#each Array(4) as _}
        <div class="skel-card skeleton"></div>
      {/each}
    </div>
  {:else if stats}
    <section class="section-block">
      <h2 class="page-title" style="font-size: var(--text-xl); margin: 0 0 var(--space-4)">
        Finding quality
      </h2>
      <div class="stats-row">
        <StatsCard
          label="Accept rate"
          value={stats.trust?.accept_rate != null
            ? `${Math.round(stats.trust.accept_rate)}%`
            : "—"}
          hint="Findings not dismissed"
          tone={acceptTone(stats.trust?.accept_rate)}
        />
        <StatsCard
          label="FP proxy"
          value={stats.trust?.fp_proxy_ratio != null
            ? Number(stats.trust.fp_proxy_ratio).toFixed(2)
            : "—"}
          hint="Dismissals ÷ Tier-1"
          tone={fpTone(stats.trust?.fp_proxy_ratio)}
        />
        <StatsCard label="Tier-1 findings" value={stats.trust?.tier1_findings ?? 0} tone="info" />
        <StatsCard label="Dismissals" value={stats.trust?.dismissals ?? 0} />
        <StatsCard
          label="LLM spend (est.)"
          value={stats.llm?.spend_usd_last_day != null
            ? `$${Number(stats.llm.spend_usd_last_day).toFixed(3)}`
            : stats.llm?.spend_usd_estimate != null
              ? `$${Number(stats.llm.spend_usd_estimate).toFixed(3)}`
              : "—"}
          hint={stats.llm?.daily_budget_usd > 0
            ? `last day · budget $${Number(stats.llm.daily_budget_usd).toFixed(2)}`
            : `${stats.llm?.requests ?? 0} requests · last day`}
        />
      </div>
    </section>

    {#if stats.analytics}
      <section class="section-block">
        <h2 class="page-title" style="font-size: var(--text-xl); margin: 0 0 var(--space-4)">
          Review analytics
        </h2>
        <div class="stats-row">
          <StatsCard
            label="Reviews (7d)"
            value={stats.analytics.weekly_digest?.reviews ?? stats.reviews_last_7_days ?? 0}
          />
          <StatsCard label="Findings (7d)" value={stats.analytics.findings_last_7_days ?? 0} />
          <StatsCard
            label="Dismiss rate (7d)"
            value={stats.analytics.dismiss_rate_last_7_days != null
              ? `${Math.round(stats.analytics.dismiss_rate_last_7_days)}%`
              : "—"}
            hint="Dismissals ÷ findings this week"
          />
          <StatsCard
            label="Dismissals (7d)"
            value={stats.analytics.weekly_digest?.dismissals ?? stats.dismissals_last_7_days ?? 0}
          />
        </div>

        {#if (stats.analytics.reviews_by_day ?? []).length > 0}
          <div class="analytics-panel" style="margin-top: var(--space-5)">
            <h3 class="section-heading" style="font-size: var(--text-base)">Reviews / day (14d)</h3>
            <div class="analytics-bars" role="img" aria-label="Reviews per day">
              {#each stats.analytics.reviews_by_day as row}
                {@const max = Math.max(
                  ...stats.analytics.reviews_by_day.map((r) => r.reviews || 0),
                  1,
                )}
                <div class="analytics-bar-col" title={`${row.day}: ${row.reviews}`}>
                  <div
                    class="analytics-bar"
                    style={`height: ${Math.max(4, Math.round((row.reviews / max) * 96))}px`}
                  ></div>
                  <span class="analytics-bar-label">{String(row.day).slice(5)}</span>
                </div>
              {/each}
            </div>
          </div>
        {/if}

        {#if (stats.analytics.findings_by_detector ?? []).length > 0}
          <div class="analytics-panel" style="margin-top: var(--space-5)">
            <h3 class="section-heading" style="font-size: var(--text-base)">
              Findings by detector
            </h3>
            <ul class="analytics-detectors">
              {#each stats.analytics.findings_by_detector as d}
                {@const maxD = Math.max(
                  ...stats.analytics.findings_by_detector.map((x) => x.count || 0),
                  1,
                )}
                <li>
                  <span class="analytics-det-name">{d.detector}</span>
                  <div class="analytics-det-track">
                    <div
                      class="analytics-det-fill"
                      style={`width: ${Math.round((d.count / maxD) * 100)}%`}
                    ></div>
                  </div>
                  <span class="analytics-det-count">{d.count}</span>
                </li>
              {/each}
            </ul>
          </div>
        {/if}
      </section>
    {:else}
      <EmptyState message="Analytics rollups unavailable yet — run a few reviews first." />
    {/if}
  {:else}
    <EmptyState message="No stats available" />
  {/if}
</AppShell>
