<script>
  import BrandMark from "./BrandMark.svelte";
  import WizardProgress from "./WizardProgress.svelte";

  /** @type {{ current?: string, status?: Record<string, boolean> | null, title?: string, subtitle?: string, showProgress?: boolean }} */
  let {
    current = "",
    status = null,
    title = "",
    subtitle = "",
    showProgress = true,
    children,
  } = $props();
</script>

<div class="wizard-shell">
  <header class="wizard-shell-header">
    <div class="wizard-brand">
      <BrandMark size={32} />
      <span class="wizard-brand-name">Codasaurus</span>
    </div>
  </header>

  <div class="wizard-card wizard-card--shell">
    {#if showProgress && current}
      <WizardProgress {current} {status} />
    {/if}

    {#if title}
      <h1 class="wizard-title">{title}</h1>
    {/if}
    {#if subtitle}
      <p class="wizard-subtitle">{subtitle}</p>
    {/if}

    <div class="wizard-body">
      {@render children?.()}
    </div>
  </div>
</div>
