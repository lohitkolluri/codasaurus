<script>
  import { onMount } from "svelte";
  import { push } from "svelte-spa-router";
  import BrandMark from "../lib/BrandMark.svelte";

  let setupComplete = $state(false);
  let checking = $state(true);
  let statusError = $state("");

  onMount(async () => {
    try {
      const res = await fetch("/api/setup/status", { credentials: "same-origin" });
      const status = res.ok ? await res.json().catch(() => null) : null;
      if (!res.ok || !status) {
        // Ambiguous: don't pretend setup is complete.
        statusError = "Could not verify setup status.";
        setupComplete = false;
        return;
      }
      if (!status.complete) {
        push("/setup");
        return;
      }
      setupComplete = true;
    } catch {
      statusError = "Could not reach the server.";
      setupComplete = false;
    } finally {
      checking = false;
    }
  });
</script>

{#if checking}
  <div class="landing landing-loading">
    <p class="landing-load-msg">Loading…</p>
  </div>
{:else if setupComplete}
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
      <p class="landing-eyebrow">GitHub App</p>
      <h1>Codasaurus</h1>
      <p class="landing-lead">
        Self-hosted PR review. Detectors run on every diff; your LLM stays optional (BYOK).
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
      <p>AGPL · free to self-host · data stays in your Postgres</p>
    </footer>
  </div>
{:else}
  <div class="landing landing-loading">
    <p class="landing-load-msg">{statusError || "Setup incomplete."}</p>
    <div class="landing-ctas" style="justify-content:center;margin-top:16px">
      <a class="landing-cta" href="#/setup">Continue setup</a>
      <a class="landing-ghost" href="#/login">Sign in</a>
    </div>
  </div>
{/if}
