<script>
  import { onMount } from "svelte";
  import { push } from "svelte-spa-router";
  import { api } from "../../stores/api.js";

  let status = $state("exchanging");
  let errorMsg = $state("");
  let installUrl = $state("");

  onMount(async () => {
    const params = new URLSearchParams(window.location.search);
    const code = params.get("code");

    if (!code) {
      status = "error";
      errorMsg = "No authorization code received from GitHub.";
      return;
    }

    try {
      const data = await api.post("/api/setup/github/callback", { code });
      status = "success";
      installUrl = data.install_url;
      // Notify the original tab that setup is complete
      if (window.opener && !window.opener.closed) {
        window.opener.location.href = "/setup/github";
      }
    } catch (err) {
      status = "error";
      errorMsg = err.message || "Failed to complete GitHub App setup.";
    }
  });

  function goToInstall() {
    if (installUrl) window.open(installUrl, "_blank");
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

  {#if status === "exchanging"}
    <p>Exchanging code with GitHub…</p>
  {:else if status === "error"}
    <div class="error-box">
      <p>{errorMsg}</p>
    </div>
    <button onclick={() => push("/setup/github")}>Back to GitHub Setup</button>
  {:else}
    <div class="success-box">
      <p>GitHub App created and credentials saved!</p>
    </div>
    <p style="font-size:13px;color:var(--text-muted)">
      Next, install the app on your repositories so Codasaurus can review PRs.
    </p>
    <button onclick={goToInstall} style="width:100%;margin-bottom:8px">
      Install App on Repositories
    </button>
    <button onclick={handleNext} class="primary" style="width:100%">
      Skip to Next Step
    </button>
  {/if}
</div>
