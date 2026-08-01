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
      if (status && !status.complete) {
        push("/setup");
        return;
      }
      setupComplete = true;
    } catch {
      setupComplete = true;
    } finally {
      checking = false;
    }
  });
</script>

{#if !checking && setupComplete}
  <div class="landing">
    <div class="landing-stage" aria-hidden="true">
      <div class="landing-glow landing-glow-a"></div>
      <div class="landing-glow landing-glow-b"></div>
      <div class="landing-grid"></div>
      <p class="landing-wordmark">CODASAURUS</p>
    </div>

    <header class="landing-nav">
      <a href="#/" class="landing-brand">
        <BrandMark size={24} />
        <span>Codasaurus</span>
      </a>
      <nav class="landing-nav-actions">
        <a
          href="https://github.com/lohitkolluri/codasaurus"
          target="_blank"
          rel="noopener noreferrer">GitHub</a
        >
        <a class="landing-nav-signin" href="#/login">Sign in</a>
      </nav>
    </header>

    <main class="landing-hero">
      <p class="landing-eyebrow">Self-hosted GitHub App</p>
      <h1>Codasaurus</h1>
      <p class="landing-lead">
        PR review that reads the diff — Tier-1 detectors first, your LLM only when you ask.
      </p>
      <div class="landing-ctas">
        <a class="landing-cta" href="#/login">Open dashboard</a>
        <a
          class="landing-ghost"
          href="https://github.com/lohitkolluri/codasaurus#readme"
          target="_blank"
          rel="noopener noreferrer">Read the docs</a
        >
      </div>
    </main>

    <footer class="landing-foot">
      <p>Free forever<span aria-hidden="true"> · </span>No seat tax<span aria-hidden="true"> · </span>Learns in your Postgres</p>
    </footer>
  </div>
{/if}
