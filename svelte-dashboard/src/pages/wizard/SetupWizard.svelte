<script>
  import { onMount } from "svelte";
  import { push } from "svelte-spa-router";
  import { api } from "../../stores/api.js";
  import WizardShell from "../../lib/WizardShell.svelte";
  import {
    SETUP_STEPS,
    firstIncomplete,
    completedCount,
  } from "../../lib/wizard.js";

  let status = $state(null);
  let loading = $state(true);
  let error = $state("");

  function goToStep(key) {
    const step = SETUP_STEPS.find((s) => s.key === key);
    push(step?.route ?? "/setup");
  }

  function startSetup() {
    if (status?.complete) {
      push("/");
      return;
    }
    const next = firstIncomplete(status);
    if (next) push(next.route);
  }

  onMount(async () => {
    try {
      status = await api.get("/api/setup/status");
      if (status?.complete) {
        // Already set up. send operators to login instead of trapping them.
        push("/login");
        return;
      }
    } catch (err) {
      error = err.message || "Could not check setup status";
    } finally {
      loading = false;
    }
  });
</script>

{#if loading}
  <WizardShell showProgress={false}>
    <p style="color:var(--text-muted);text-align:center;margin:48px 0">Checking setup status…</p>
  </WizardShell>
{:else if error}
  <WizardShell showProgress={false} title="Can't reach the server" subtitle={error}>
    <div class="wizard-actions" style="border:none;padding-top:0">
      <button class="primary" onclick={() => push("/setup/database")}>Start setup anyway</button>
    </div>
  </WizardShell>
{:else}
  <WizardShell
    showProgress={false}
    title="Set up Codasaurus"
    subtitle="Database, LLM (optional), GitHub App, then your first owner account."
  >
    <div class="wizard-time" aria-hidden="true">About 5 minutes. You can resume later.</div>

    <ul class="wiz-hub-list">
      {#each SETUP_STEPS as step, i}
        {@const done = !!status?.[step.key]}
        {@const next = firstIncomplete(status)?.key === step.key}
        <li>
          <button
            type="button"
            class="wiz-hub-item"
            class:done
            class:next
            onclick={() => goToStep(step.key)}
          >
            <span class="wiz-hub-badge">
              {#if done}✓{:else}{i + 1}{/if}
            </span>
            <div class="wiz-hub-text">
              <strong>
                {step.label}
                {#if step.optional}<span style="font-weight:400;color:var(--text-muted)"> · optional</span>{/if}
              </strong>
              <span>{step.desc}</span>
            </div>
            <span class="wiz-hub-meta">{done ? "Done" : step.eta}</span>
          </button>
        </li>
      {/each}
    </ul>

    <div class="wizard-actions" style="border:none;padding-top:0;margin-top:0">
      <button class="primary" style="width:100%;padding:12px 24px;font-size:15px" onclick={startSetup}>
        {completedCount(status) === 0
          ? "Get started"
          : `Continue setup (${completedCount(status)}/${SETUP_STEPS.length})`}
      </button>
    </div>

    <p class="wizard-hint" style="text-align:center;margin-top:16px">
      Already configured?
      <button type="button" class="linkish" onclick={() => push("/login")}>Sign in</button>
    </p>
  </WizardShell>
{/if}

<style>
  .linkish {
    background: none;
    border: none;
    padding: 0;
    color: var(--accent-soft, var(--accent));
    cursor: pointer;
    font-size: inherit;
    text-decoration: underline;
    text-underline-offset: 2px;
  }
</style>
