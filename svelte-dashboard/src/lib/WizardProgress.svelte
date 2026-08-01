<script>
  import { push } from "svelte-spa-router";
  import { SETUP_STEPS, stepIndex } from "./wizard.js";

  /** @type {{ current: string, status?: Record<string, boolean> | null }} */
  let { current, status = null } = $props();

  const idx = $derived(stepIndex(current));
</script>

<nav class="wiz-progress" aria-label="Setup progress">
  <ol class="wiz-progress-list">
    {#each SETUP_STEPS as step, i}
      {@const done = status ? !!status[step.key] : i < idx}
      {@const active = i === idx}
      {@const reachable = done || i <= idx || (status && SETUP_STEPS.slice(0, i).every((s) => status[s.key]))}
      <li class="wiz-progress-item" class:done class:active>
        {#if i > 0}
          <span class="wiz-progress-line" class:filled={i <= idx || done} aria-hidden="true"></span>
        {/if}
        <button
          type="button"
          class="wiz-progress-btn"
          class:done
          class:active
          disabled={!reachable && !done}
          aria-current={active ? "step" : undefined}
          title="{step.label}{step.optional ? ' (optional)' : ''}"
          onclick={() => push(step.route)}
        >
          <span class="wiz-progress-num">
            {#if done && !active}
              <svg width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true">
                <path d="M2.5 6.5L5 9l4.5-5.5" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/>
              </svg>
            {:else}
              {i + 1}
            {/if}
          </span>
          <span class="wiz-progress-label">{step.short}</span>
        </button>
      </li>
    {/each}
  </ol>
  <p class="wiz-progress-meta">
    Step {idx + 1} of {SETUP_STEPS.length}
    {#if SETUP_STEPS[idx]?.optional}
      <span class="wiz-optional"> · optional</span>
    {/if}
    {#if SETUP_STEPS[idx]?.eta}
      <span class="wiz-eta"> · {SETUP_STEPS[idx].eta}</span>
    {/if}
  </p>
</nav>
