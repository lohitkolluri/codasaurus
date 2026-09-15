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

  // Public URL (manifest webhook/callback origin)
  let publicUrl = $state("");
  let publicUrlSource = $state("auto");
  let publicUrlLocal = $state(false);
  let savingUrl = $state(false);
  let urlMsg = $state("");

  // Manual credentials form (for users who already have an app)
  let showManual = $state(false);
  let appId = $state("");
  let privateKey = $state("");
  let webhookSecret = $state("");
  let slug = $state("");
  let savingManual = $state(false);
  let manualMsg = $state("");
  let manualError = $state(false);

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

  async function refreshPublicUrl() {
    try {
      const data = await api.get("/api/setup/public-url");
      publicUrl = data.url ?? "";
      publicUrlSource = data.source ?? "auto";
      publicUrlLocal = !!data.localhost;
    } catch {
      /* ignore */
    }
  }

  async function savePublicUrl() {
    savingUrl = true;
    urlMsg = "";
    try {
      const res = await api.post("/api/setup/public-url", { url: publicUrl });
      urlMsg = res?.message || "Saved";
      await refreshPublicUrl();
    } catch (err) {
      urlMsg = err.message || "Failed to save public URL";
    } finally {
      savingUrl = false;
    }
  }

  async function saveManual() {
    savingManual = true;
    manualMsg = "";
    manualError = false;
    try {
      const body = {
        app_id: appId.trim(),
        private_key: privateKey,
        webhook_secret: webhookSecret,
      };
      if (slug.trim()) body.slug = slug.trim();
      const res = await api.post("/api/setup/github", body);
      manualMsg = res?.message || "GitHub App connected";
      showManual = false;
      await refreshStatus();
    } catch (err) {
      manualMsg = err.message || "Failed to save GitHub App credentials";
      manualError = true;
    } finally {
      savingManual = false;
    }
  }

  onMount(() => {
    refreshStatus();
    refreshPublicUrl();
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
    <div class="form-group" style="margin-bottom:12px">
      <label for="wizard-public-url">Public URL <span style="opacity:0.6">(where GitHub reaches this server)</span></label>
      <div style="display:flex;gap:8px">
        <input
          id="wizard-public-url"
          type="url"
          bind:value={publicUrl}
          placeholder="https://reviews.example.com"
          style="flex:1"
        />
        <button type="button" onclick={savePublicUrl} disabled={savingUrl}>
          {savingUrl ? "Saving…" : "Save"}
        </button>
      </div>
      {#if urlMsg}
        <p class="wizard-hint">{urlMsg}</p>
      {:else}
        <p class="wizard-hint">Detected from {publicUrlSource}. The manifest uses this for the webhook and callback URLs.</p>
      {/if}
      {#if publicUrlLocal}
        <div class="info-box" style="margin-top:8px">
          <strong style="font-size:13px">Localhost can't receive webhooks</strong>
          <p style="margin:8px 0 0;font-size:13px;opacity:0.9">
            GitHub can't reach a localhost URL. Deploy Codasaurus with a public HTTPS origin,
            or expose it temporarily with e.g. <code>cloudflared tunnel --url http://localhost:3000</code>
            and save the tunnel URL above before creating the app.
          </p>
        </div>
      {/if}
    </div>

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

    <div style="margin-top:12px">
      <button type="button" onclick={() => (showManual = !showManual)} style="padding:6px 12px;font-size:13px">
        {showManual ? "Hide manual setup" : "Already have an app? Enter details manually"}
      </button>
    </div>

    {#if showManual}
      <div style="margin-top:12px;display:flex;flex-direction:column;gap:10px">
        <div class="form-group">
          <label for="manual-app-id">App ID</label>
          <input id="manual-app-id" type="text" bind:value={appId} placeholder="123456" autocomplete="off" />
        </div>
        <div class="form-group">
          <label for="manual-private-key">Private key (.pem contents)</label>
          <textarea
            id="manual-private-key"
            bind:value={privateKey}
            rows="5"
            placeholder="-----BEGIN RSA PRIVATE KEY-----"
            spellcheck="false"
            style="font-family:monospace;font-size:12px"
          ></textarea>
        </div>
        <div class="form-group">
          <label for="manual-webhook-secret">Webhook secret</label>
          <input id="manual-webhook-secret" type="password" bind:value={webhookSecret} autocomplete="off" />
        </div>
        <div class="form-group">
          <label for="manual-slug">App slug <span style="opacity:0.6">(optional — auto-detected if blank)</span></label>
          <input id="manual-slug" type="text" bind:value={slug} placeholder="my-codasaurus" autocomplete="off" />
          <p class="wizard-hint">Find it in the app's GitHub URL: github.com/apps/&lt;slug&gt;.</p>
        </div>
        {#if manualMsg}
          <p class="wizard-hint" class:error={manualError}>{manualMsg}</p>
        {/if}
        <div>
          <button
            type="button"
            class="primary"
            onclick={saveManual}
            disabled={savingManual || !appId.trim() || !privateKey.trim() || !webhookSecret.trim()}
          >
            {savingManual ? "Verifying…" : "Verify & save"}
          </button>
        </div>
      </div>
    {/if}

    <button type="button" onclick={refreshStatus} disabled={refreshing} style="margin-top:8px">
      {refreshing ? "Checking…" : "I've finished. Refresh status"}
    </button>
  {/if}

  <div class="wizard-actions">
    <button onclick={() => push("/setup/llm")}>Back</button>
    <button class="primary" onclick={() => push("/setup/admin")} disabled={!configured}>Continue</button>
  </div>
</WizardShell>
