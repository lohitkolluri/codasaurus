<script>
  import { onMount } from "svelte";
  import { push } from "svelte-spa-router";
  import { api } from "../../stores/api.js";

  let status = $state(null);
  let loading = $state(true);

  onMount(async () => {
    try {
      status = await api.get("/api/setup/status");
      if (!status.complete) {
        if (!status.database) push("/setup/database");
        else if (!status.llm) push("/setup/llm");
        else if (!status.github) push("/setup/github");
        else if (!status.admin) push("/setup/admin");
      }
    } catch {
      // ignore — render the page anyway
    } finally {
      loading = false;
    }
  });
</script>

<div class="wizard-card" style="text-align:center;padding-top:80px">
  {#if loading}
    <p style="color:var(--text-muted)">Verifying setup…</p>
  {:else}
    <div class="step-indicator">
      <span class="step-dot completed"></span>
      <span class="step-dot completed"></span>
      <span class="step-dot completed"></span>
      <span class="step-dot completed"></span>
    </div>

    <h2 style="font-size:24px;font-weight:700;margin-bottom:12px">Setup Complete</h2>
    <p style="color:var(--text-muted);margin-bottom:40px;max-width:400px;margin-left:auto;margin-right:auto">
      Codasaurus is configured and ready. Your database, LLM, GitHub integration, and admin account have been set up.
    </p>

    <div style="text-align:left;margin-bottom:40px;max-width:400px;margin-left:auto;margin-right:auto">
      <div style="padding:8px 0;border-bottom:1px solid var(--border);display:flex;justify-content:space-between">
        <span style="color:var(--text-muted)">Database</span>
        <span style="font-weight:500;color:var(--success)">Configured</span>
      </div>
      <div style="padding:8px 0;border-bottom:1px solid var(--border);display:flex;justify-content:space-between">
        <span style="color:var(--text-muted)">LLM</span>
        <span style="font-weight:500;color:var(--success)">Configured</span>
      </div>
      <div style="padding:8px 0;border-bottom:1px solid var(--border);display:flex;justify-content:space-between">
        <span style="color:var(--text-muted)">GitHub</span>
        <span style="font-weight:500;color:var(--success)">Configured</span>
      </div>
      <div style="padding:8px 0;display:flex;justify-content:space-between">
        <span style="color:var(--text-muted)">Admin</span>
        <span style="font-weight:500;color:var(--success)">Created</span>
      </div>
    </div>

    <button class="primary" style="font-size:16px;padding:12px 40px" onclick={() => push("/login")}>
      Go to Dashboard
    </button>
  {/if}
</div>
