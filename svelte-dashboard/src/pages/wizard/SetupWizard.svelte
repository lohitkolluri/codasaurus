<script>
  import { onMount } from "svelte";
  import { push } from "svelte-spa-router";
  import { api } from "../../stores/api.js";

  let status = $state(null);
  let loading = $state(true);
  let error = $state("");

  const steps = [
    { key: "database", label: "Database", desc: "Configure your database connection" },
    { key: "llm", label: "LLM", desc: "Set up AI-powered code review" },
    { key: "github", label: "GitHub App", desc: "Connect your repositories" },
    { key: "admin", label: "Admin Account", desc: "Create your login credentials" },
  ];

  function completedCount() {
    if (!status) return 0;
    return steps.filter((s) => status[s.key]).length;
  }

  function firstIncomplete() {
    if (!status) return null;
    for (const s of steps) {
      if (!status[s.key]) return s;
    }
    return null;
  }

  function startSetup() {
    const next = firstIncomplete();
    if (!next) return;
    goToStep(next.key);
  }

  function goToStep(key) {
    const routes = { database: "/setup/database", llm: "/setup/llm", github: "/setup/github", admin: "/setup/admin" };
    push(routes[key] ?? "/setup");
  }

  onMount(async () => {
    try {
      status = await api.get("/api/setup/status");
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
    <div style="max-width:400px;margin:0 auto">
      {#each steps as step, i}
        <div class="step-row" onclick={() => goToStep(step.key)} role="button" tabindex="0" onkeydown={(e) => e.key === 'Enter' && goToStep(step.key)}>
          <div style="text-align:left">
            <div style="font-size:14px;font-weight:500">{step.label}</div>
            <div style="font-size:12px;color:var(--text-muted)">{step.desc}</div>
          </div>
          <span style="font-size:18px">
            {#if status[step.key]}
              <span style="color:var(--accent-soft)">✓</span>
            {:else}
              <span style="color:var(--text-muted);font-weight:600">{i + 1}</span>
            {/if}
          </span>
        </div>
      {/each}
    </div>

    <button class="primary" style="font-size:16px;padding:12px 40px;margin-top:32px" onclick={startSetup}>
      {status.complete ? "Go to Login" : `Continue Setup (${completedCount()}/4)`}
    </button>
  {/if}
</div>

<style>
  .step-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 0;
    border-bottom: 1px solid var(--border-light);
    cursor: pointer;
    transition: opacity 0.15s;
  }
  .step-row:hover {
    opacity: 0.7;
  }
</style>
