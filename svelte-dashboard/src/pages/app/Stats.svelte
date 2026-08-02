<script>
  import { onMount } from "svelte";
  import { link } from "svelte-spa-router";
  import { api } from "../../stores/api.js";
  import AppShell from "../../lib/AppShell.svelte";
  import StatsCard from "../../lib/StatsCard.svelte";
  import TrendChart from "../../lib/TrendChart.svelte";
  import OutcomeChart from "../../lib/OutcomeChart.svelte";
  import Pagination from "../../lib/Pagination.svelte";
  import LoadingSpinner from "../../lib/LoadingSpinner.svelte";
  import EmptyState from "../../lib/EmptyState.svelte";
  import ErrorState from "../../lib/ErrorState.svelte";

  const DETECTOR_PAGE = 8;

  let stats = $state(null);
  let loading = $state(true);
  let error = $state("");
  let detectorPage = $state(1);

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

  function passTone(rate) {
    if (rate == null) return "";
    if (rate >= 80) return "success";
    if (rate >= 60) return "warning";
    return "danger";
  }

  function pctDelta(curr, prev) {
    if (prev == null || prev === 0) {
      if (curr == null || curr === 0) return null;
      return 100;
    }
    return ((curr - prev) / prev) * 100;
  }

  function absDelta(curr, prev) {
    if (curr == null || prev == null) return null;
    return curr - prev;
  }

  function fmtRate(rate) {
    if (rate == null) return "n/a";
    return `${Math.round(rate)}%`;
  }

  function fmtRatio(ratio) {
    if (ratio == null) return "n/a";
    return Number(ratio).toFixed(2);
  }

  function fmtSpend(usd) {
    if (usd == null) return "$0.000";
    return `$${Number(usd).toFixed(3)}`;
  }

  let series = $derived(stats?.analytics?.reviews_by_day ?? []);
  let detectors = $derived(stats?.analytics?.findings_by_detector ?? []);
  let concerns = $derived(stats?.analytics?.findings_by_concern ?? []);
  let outcomes = $derived(stats?.analytics?.outcomes_7d ?? { passed: 0, failed: 0, other: 0 });

  let reviewSpark = $derived(series.map((r) => r.reviews || 0));
  let findingSpark = $derived(series.map((r) => r.findings || 0));
  let chartLabels = $derived(series.map((r) => dayLabel(r.day)));
  let chartReviews = $derived(series.map((r) => r.reviews || 0));
  let chartFindings = $derived(series.map((r) => r.findings || 0));

  let seriesMax = $derived(
    Math.max(0, ...series.map((r) => Math.max(r.reviews || 0, r.findings || 0))),
  );
  let chartHasActivity = $derived(seriesMax > 0);

  let detectorPages = $derived(Math.max(1, Math.ceil(detectors.length / DETECTOR_PAGE)));
  let detectorPageSafe = $derived(Math.min(Math.max(1, detectorPage), detectorPages));
  let pageDetectors = $derived.by(() => {
    const start = (detectorPageSafe - 1) * DETECTOR_PAGE;
    return detectors.slice(start, start + DETECTOR_PAGE);
  });

  let outcomeTotal = $derived(
    (outcomes.passed || 0) + (outcomes.failed || 0) + (outcomes.other || 0),
  );

  let insights = $derived.by(() => {
    if (!stats?.analytics) return [];
    const out = [];
    const a = stats.analytics;
    const pass = a.pass_rate_7d;
    const dismiss = a.dismiss_rate_last_7_days;
    const accept = stats.trust?.accept_rate;
    const reviews = a.weekly_digest?.reviews ?? 0;
    const prev = a.reviews_prev_7_days ?? 0;

    if (reviews === 0) {
      out.push({
        tone: "info",
        text: "No reviews in the last 7 days. Enable a repo and open a PR.",
        href: "#/app/repos",
        linkLabel: "Repositories",
      });
    } else if (prev > 0 && reviews > prev * 1.25) {
      out.push({
        tone: "success",
        text: `Review volume is up ${Math.round(((reviews - prev) / prev) * 100)}% vs the prior week.`,
      });
    } else if (prev > 0 && reviews < prev * 0.75) {
      out.push({
        tone: "warning",
        text: `Review volume is down ${Math.round(((prev - reviews) / prev) * 100)}% vs the prior week.`,
      });
    }

    if (pass != null && pass < 60) {
      out.push({
        tone: "danger",
        text: `Pass rate is ${Math.round(pass)}% this week. Check detectors and review policy.`,
        href: "#/app/settings",
        linkLabel: "Settings",
      });
    } else if (pass != null && pass >= 85) {
      out.push({
        tone: "success",
        text: `Pass rate is ${Math.round(pass)}% over the last 7 days.`,
      });
    }

    if (dismiss != null && dismiss >= 25) {
      out.push({
        tone: "warning",
        text: `Dismiss rate is ${Math.round(dismiss)}%. Noise may be high; review learned ignore rules.`,
        href: "#/app/settings/learning",
        linkLabel: "Learning",
      });
    }

    if (accept != null && accept < 70) {
      out.push({
        tone: "warning",
        text: `Finding accept proxy is ${Math.round(accept)}% (30d). Check FP proxy and dismissals.`,
      });
    }

    return out.slice(0, 3);
  });

  function dayLabel(day) {
    if (!day) return "";
    const s = String(day);
    return s.length >= 10 ? s.slice(5) : s;
  }
</script>

<AppShell title="Stats">
  <div class="page-toolbar compact stats-hero">
    <div>
      <p class="eyebrow">Analytics</p>
      <h1 class="page-title">Stats</h1>
      <p class="page-description">
        Week-over-week KPIs, then trends and detector mix.
      </p>
    </div>
    <div class="toolbar-actions">
      <a class="btn" href="#/app/dashboard" use:link>Dashboard</a>
      <a class="btn" href="#/app/reviews" use:link>Reviews</a>
    </div>
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
    {#if insights.length > 0}
      <ul class="stats-insights" aria-label="Insights">
        {#each insights as tip}
          <li class="stats-insight" class:tone-success={tip.tone === "success"} class:tone-warning={tip.tone === "warning"} class:tone-danger={tip.tone === "danger"} class:tone-info={tip.tone === "info"}>
            {tip.text}
            {#if tip.href && tip.linkLabel}
              <a class="btn sm" href={tip.href} use:link>{tip.linkLabel}</a>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}

    <section class="section-block">
      <h2 class="stats-section-title">This week</h2>
      <div class="stats-row">
        <StatsCard
          label="Reviews (7d)"
          value={stats.analytics?.weekly_digest?.reviews ?? stats.reviews_last_7_days ?? 0}
          delta={pctDelta(
            stats.analytics?.weekly_digest?.reviews ?? stats.reviews_last_7_days ?? 0,
            stats.analytics?.reviews_prev_7_days,
          )}
          deltaLabel="% vs prior week"
          spark={reviewSpark}
        />
        <StatsCard
          label="Findings (7d)"
          value={stats.analytics?.findings_last_7_days ?? 0}
          delta={pctDelta(
            stats.analytics?.findings_last_7_days ?? 0,
            stats.analytics?.findings_prev_7_days,
          )}
          deltaLabel="% vs prior week"
          spark={findingSpark}
        />
        <StatsCard
          label="Pass rate (7d)"
          value={fmtRate(stats.analytics?.pass_rate_7d)}
          delta={absDelta(stats.analytics?.pass_rate_7d, stats.analytics?.pass_rate_prev_7d)}
          deltaLabel="pp vs prior week"
          tone={passTone(stats.analytics?.pass_rate_7d)}
          hint={stats.pass_rate != null ? `30d avg ${Math.round(stats.pass_rate)}%` : "No reviews yet"}
        />
        <StatsCard
          label="Dismiss rate (7d)"
          value={fmtRate(stats.analytics?.dismiss_rate_last_7_days)}
          delta={absDelta(
            stats.analytics?.weekly_digest?.dismissals ?? 0,
            stats.analytics?.dismissals_prev_7_days,
          )}
          deltaLabel="dismissals Δ"
          hint="Dismissals ÷ (dismissals + findings)"
        />
      </div>
    </section>

    <section class="section-block">
      <h2 class="stats-section-title">Finding quality</h2>
      <p class="stats-section-note">
        Accept rate and FP proxy are dismissal-based heuristics (not billing). LLM spend is an estimate from event logs.
      </p>
      <div class="stats-row">
        <StatsCard
          label="Accept rate"
          value={fmtRate(stats.trust?.accept_rate)}
          hint="Not dismissed · 30d"
          tone={acceptTone(stats.trust?.accept_rate)}
        />
        <StatsCard
          label="FP proxy"
          value={fmtRatio(stats.trust?.fp_proxy_ratio ?? 0)}
          hint="Dismissals ÷ Tier-1"
          tone={fpTone(stats.trust?.fp_proxy_ratio)}
        />
        <StatsCard label="Tier-1 findings" value={stats.trust?.tier1_findings ?? 0} tone="info" />
        <StatsCard
          label="LLM spend (est.)"
          value={fmtSpend(
            stats.llm?.spend_usd_last_day ?? stats.llm?.spend_usd_estimate ?? 0,
          )}
          hint={stats.llm?.daily_budget_usd > 0
            ? `last day · budget $${Number(stats.llm.daily_budget_usd).toFixed(2)}`
            : `${stats.llm?.requests ?? 0} requests · last day`}
        />
      </div>
    </section>

    {#if stats.analytics}
      <section class="section-block">
        <h2 class="stats-section-title">Trends (14d)</h2>
        <div class="stats-chart-grid">
          <div class="analytics-panel chart-card">
            <div class="chart-card-head">
              <h3 class="section-heading">Reviews &amp; findings / day</h3>
            </div>
            <TrendChart
              labels={chartLabels}
              reviews={chartReviews}
              findings={chartFindings}
              empty={!chartHasActivity}
            >
              {#snippet action()}
                <a class="btn sm" href="#/app/repos" use:link>Enable a repository</a>
              {/snippet}
            </TrendChart>
          </div>

          <div class="analytics-panel chart-card">
            <div class="chart-card-head">
              <h3 class="section-heading">Outcomes (7d)</h3>
            </div>
            <OutcomeChart
              passed={outcomes.passed || 0}
              failed={outcomes.failed || 0}
              other={outcomes.other || 0}
              empty={outcomeTotal === 0}
            >
              {#snippet action()}
                <a class="btn sm" href="#/app/reviews" use:link>Open reviews</a>
              {/snippet}
            </OutcomeChart>
          </div>
        </div>
      </section>

      {#if concerns.length > 0}
        <section class="section-block">
          <h2 class="stats-section-title">Findings by concern (30d)</h2>
          <div class="analytics-panel chart-card">
            <ul class="analytics-detectors">
              {#each concerns as c, i}
                <li>
                  <span class="analytics-det-name">{c.concern}</span>
                  <div class="analytics-det-track">
                    <div
                      class={`analytics-det-fill tone-${i % 8}`}
                      style={`width: ${Math.min(100, Math.max(4, Math.round(c.share_pct ?? 0)))}%`}
                    ></div>
                  </div>
                  <span class="analytics-det-count">
                    {c.count}
                    <span class="muted">{c.share_pct != null ? `${Math.round(c.share_pct)}%` : ""}</span>
                  </span>
                </li>
              {/each}
            </ul>
          </div>
        </section>
      {/if}

      {#if detectors.length > 0}
        <section class="section-block">
          <h2 class="stats-section-title">Findings by detector (30d)</h2>
          <div class="analytics-panel chart-card analytics-detectors-card">
            <ul class="analytics-detectors scroll-thin">
              {#each pageDetectors as d, i}
                <li>
                  <span class="analytics-det-name" title={d.detector}>{d.detector}</span>
                  <div class="analytics-det-track">
                    <div
                      class={`analytics-det-fill tone-${i % 8}`}
                      style={`width: ${Math.min(100, Math.max(4, Math.round(d.share_pct ?? 0)))}%`}
                    ></div>
                  </div>
                  <span class="analytics-det-count">
                    {d.count}
                    <span class="muted">{d.share_pct != null ? `${Math.round(d.share_pct)}%` : ""}</span>
                  </span>
                </li>
              {/each}
            </ul>
            {#if detectors.length > DETECTOR_PAGE}
              <div class="detector-page-meta">
                <span>
                  {detectors.length} detectors · page {detectorPageSafe} of {detectorPages}
                </span>
                <Pagination
                  page={detectorPageSafe}
                  totalPages={detectorPages}
                  onChange={(p) => (detectorPage = p)}
                />
              </div>
            {/if}
          </div>
        </section>
      {/if}
    {:else}
      <EmptyState message="No analytics yet. Run a few reviews first." />
    {/if}
  {:else}
    <EmptyState message="No stats available" />
  {/if}
</AppShell>

<style>
  .stats-hero {
    align-items: flex-end;
  }
  .stats-section-note {
    font-size: 13px;
    color: var(--text-muted);
    margin: 0 0 12px;
    max-width: 52rem;
    line-height: 1.45;
  }
  .stats-section-title {
    margin: 0 0 var(--space-4);
    font-size: var(--text-xl);
    font-weight: var(--weight-semibold);
    letter-spacing: -0.02em;
  }
  .section-block {
    margin-bottom: var(--space-8);
  }
  .stats-insights {
    list-style: none;
    margin: 0 0 var(--space-6);
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .stats-insight {
    margin: 0;
    padding: 10px 14px;
    border-radius: var(--radius-md);
    border: 1px solid var(--border);
    font-size: var(--text-sm);
    background: color-mix(in srgb, var(--bg-secondary) 55%, transparent);
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 8px 12px;
  }
  .stats-insight.tone-success {
    border-color: color-mix(in srgb, var(--success) 35%, var(--border));
  }
  .stats-insight.tone-warning {
    border-color: color-mix(in srgb, var(--warning) 40%, var(--border));
  }
  .stats-insight.tone-danger {
    border-color: color-mix(in srgb, var(--error) 35%, var(--border));
  }
  .empty-note {
    margin: var(--space-4) 0 0;
    font-size: var(--text-sm);
    color: var(--text-muted);
  }
  .muted {
    color: var(--text-muted);
    font-weight: var(--weight-regular);
  }
</style>
