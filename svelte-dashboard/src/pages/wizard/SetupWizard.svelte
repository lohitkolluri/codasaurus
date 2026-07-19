<script>
  import { onMount } from "svelte";
  import { push } from "svelte-spa-router";
  import { api } from "../../stores/api.js";

  let status = $state(null);
  let loading = $state(true);
  let error = $state("");

  onMount(async () => {
    try {
      status = await api.get("/api/setup/status");
      if (status.complete) {
        push("/login");
        return;
      }
      if (!status.database) push("/setup/database");
      else if (!status.llm) push("/setup/llm");
      else if (!status.github) push("/setup/github");
      else if (!status.admin) push("/setup/admin");
      else push("/login");
    } catch (err) {
      error = err.message || "Could not check setup status";
    } finally {
      loading = false;
    }
  });
</script>

<div class="wizard-card" style="text-align:center;padding-top:120px">
  <h1 style="font-size:48px;font-weight:700;margin-bottom:8px">Codasaurus</h1>
  <p style="font-size:16px;color:var(--text-muted);margin-bottom:48px">AI Code Review Platform</p>

  {#if loading}
    <p style="color:var(--text-muted)">Checking setup status…</p>
  {:else if error}
    <div class="error-state" style="margin-bottom:16px">{error}</div>
    <button class="primary" onclick={() => push("/setup/database")}>Start Setup</button>
  {:else}
    <div class="step-indicator">
      <span class="step-dot active"></span>
      <span class="step-dot"></span>
      <span class="step-dot"></span>
      <span class="step-dot"></span>
    </div>
    <button class="primary" style="font-size:16px;padding:12px 40px" onclick={() => push("/setup/database")}>
      Get Started
    </button>
  {/if}
</div>
