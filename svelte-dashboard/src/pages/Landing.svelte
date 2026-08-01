<script>
  import { onMount } from "svelte";
  import { push } from "svelte-spa-router";
  import BrandMark from "../lib/BrandMark.svelte";

  let setupComplete = $state(false);
  let checking = $state(true);

  onMount(async () => {
    try {
      const status = await fetch("/api/setup/status", { credentials: "same-origin" }).then((r) =>
        r.ok ? r.json() : null,
      );
      setupComplete = !!status?.complete;
      if (!setupComplete) {
        push("/setup");
        return;
      }
    } catch {
      /* stay on landing */
    } finally {
      checking = false;
    }
  });
</script>

{#if !checking && setupComplete}
  <div class="landing">
    <div class="landing-bg" aria-hidden="true"></div>
    <header class="landing-nav">
      <a href="#/" class="landing-brand">
        <BrandMark size={28} />
        <span>Codasaurus</span>
      </a>
      <div class="landing-nav-actions">
        <a href="https://github.com/lohitkolluri/codasaurus" target="_blank" rel="noopener noreferrer"
          >GitHub</a
        >
        <a class="landing-cta-secondary" href="#/login">Sign in</a>
      </div>
    </header>

    <main class="landing-hero">
      <p class="landing-kicker">Self-hosted · Free forever · BYOK optional</p>
      <h1 class="landing-title">Codasaurus</h1>
      <p class="landing-lead">
        A GitHub App that reviews PRs like a senior who actually reads the diff — on your Postgres,
        with Tier-1 detectors first and your LLM keys only when you want them.
      </p>
      <div class="landing-ctas">
        <a class="landing-cta" href="#/login">Open dashboard</a>
        <a
          class="landing-cta-secondary"
          href="https://github.com/lohitkolluri/codasaurus#readme"
          target="_blank"
          rel="noopener noreferrer">Run for $0</a
        >
      </div>
    </main>

    <section class="landing-points">
      <article>
        <h2>No seat tax</h2>
        <p>One binary, invite links for your team, permanent free host + Postgres paths documented.</p>
      </article>
      <article>
        <h2>Tier-1 first</h2>
        <p>Secrets, phantom deps, OSV, IaC, and stale APIs work with the LLM off.</p>
      </article>
      <article>
        <h2>Learns in-place</h2>
        <p>Dismissals, comment mining, and 👎 reactions feed your Postgres learning store.</p>
      </article>
    </section>
  </div>
{/if}
