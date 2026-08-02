<script>
  import { onMount, onDestroy } from "svelte";
  import { push } from "svelte-spa-router";
  import { api } from "../../stores/api.js";
  import WizardShell from "../../lib/WizardShell.svelte";

  let configured = $state(false);
  let checking = $state(true);
  let status = $state(null);
  let poll = null;
  let refreshing = $state(false);

  async function refreshStatus() {
    refreshing = true;
    try {
      status = await api.get("/api/setup/status");
      configured = !!status.github;
    } catch {
      /* ignore */
    } finally {
      checking = false;
      refreshing = false;
    }
  }

  onMount(() => {
    refreshStatus();
    poll = setInterval(() => {
      if (!configured) refreshStatus();
    }, 2500);
    const onFocus = () => refreshStatus();
    window.addEventListener("focus", onFocus);
    return () => {
      if (poll) clearInterval(poll);
      window.removeEventListener("focus", onFocus);
    };
  });

  onDestroy(() => {
    if (poll) clearInterval(poll);
  });

  $effect(() => {
    if (configured && poll) {
      clearInterval(poll);
      poll = null;
    }
  });

  function openManifest() {
    window.open("/api/setup/github/manifest-page", "_blank", "noopener,noreferrer");
  }
</script>

<WizardShell
  current="github"
  {status}
  title="Connect GitHub"
  subtitle="Create a GitHub App in one click. Codasaurus gets webhook + PR permissions. You keep the keys on your server."
>
  {#if checking}
    <p style="color:var(--text-muted)">Checking GitHub App status…</p>
  {:else if configured}
    <div class="success-box">
      <strong>GitHub App connected</strong>
      <p style="margin:8px 0 0;font-size:13px;opacity:0.9">
        Next you'll create an admin login. After that, install the App on the repos you want reviewed.
      </p>
    </div>
    <div class="info-box" style="margin-top:12px">
      <strong style="font-size:13px">Optional: App icon</strong>
      <p style="margin:8px 0 0;font-size:13px;opacity:0.9">
        GitHub’s create-from-manifest flow cannot set a logo. After setup, open
        <strong>GitHub → Settings → Developer settings → GitHub Apps → your app → Display information</strong>
        and upload
        <a href="/branding/logo.png" target="_blank" rel="noopener noreferrer">/branding/logo.png</a>
        (512×512 PNG).
      </p>
    </div>
  {:else}
    <button class="primary" onclick={openManifest} style="width:100%;margin-bottom:12px;padding:12px">
      Create GitHub App
    </button>
    <p class="wizard-hint" style="margin-bottom:16px">
      Opens GitHub in a new tab. Finish the form. Credentials save automatically when you return.
      {#if refreshing}
        <span> Checking…</span>
      {/if}
    </p>

    <div class="info-box">
      <strong style="font-size:13px">What happens</strong>
      <ul>
        <li>Manifest pre-fills webhook URL, permissions, and callbacks</li>
        <li>Private key + App ID are stored on this server only</li>
        <li>Install on orgs/repos after setup completes</li>
      </ul>
    </div>

    <button type="button" onclick={refreshStatus} disabled={refreshing} style="margin-top:8px">
      {refreshing ? "Checking…" : "I've finished. Refresh status"}
    </button>
  {/if}

  <div class="wizard-actions">
    <button onclick={() => push("/setup/llm")}>Back</button>
    <button class="primary" onclick={() => push("/setup/admin")} disabled={!configured}>Continue</button>
  </div>
</WizardShell>
