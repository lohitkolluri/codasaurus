<script>
  import { onMount } from "svelte";
  import { push } from "svelte-spa-router";
  import { api } from "../../stores/api.js";

  let configured = $state(false);
  let checking = $state(true);
  onMount(async () => {
    try {
      const status = await api.get("/api/setup/status");
      configured = status.github;
    } catch {
      // ignore
    } finally {
      checking = false;
    }
  });

  async function openManifest() {
    // Opens a page that auto-submits the manifest to GitHub via POST form.
    // This is the officially documented flow — GET with ?manifest= is unreliable.
    window.open("/api/setup/github/manifest-page", "_blank");
  }

  function handleNext() {
    push("/setup/admin");
  }
</script>

<div class="wizard-card">
  <div class="step-indicator">
    <span class="step-dot completed"></span>
    <span class="step-dot completed"></span>
    <span class="step-dot active"></span>
    <span class="step-dot"></span>
  </div>
  <p class="wizard-step-label">Step 3 of 4 — GitHub App</p>

  {#if checking}
    <p style="color:var(--text-muted)">Checking…</p>
  {:else if configured}
    <div class="success-box">
      <p>GitHub App is configured.</p>
    </div>
  {:else}
    <button onclick={openManifest} style="width:100%;margin-bottom:12px">
      Create GitHub App on GitHub
    </button>
    <p style="font-size:13px;color:var(--text-muted);margin-bottom:16px">
      A new browser tab will open. Complete the GitHub App creation form, then you will be redirected back here.
    </p>

    <div class="info-box">
      <strong style="font-size:13px">After the GitHub redirect:</strong>
      <ul style="font-size:13px;margin:8px 0 0 0;padding-left:20px">
        <li>Your credentials are saved automatically</li>
        <li>Install the app on your repositories</li>
        <li>Return here and click Next</li>
      </ul>
    </div>
  {/if}

  <div class="wizard-actions" style="margin-top:24px">
    <button onclick={() => push("/setup/llm")}>Back</button>
    <button class="primary" onclick={handleNext} disabled={!configured}>Next Step</button>
  </div>
</div>
