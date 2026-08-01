<script>
  let { message = "No data found.", actionLabel = "", onAction = null } = $props();
</script>

<div class="empty-state" role="status">
  <div class="empty-glyph" aria-hidden="true">
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
      <polyline points="14 2 14 8 20 8" />
      <line x1="9" y1="13" x2="15" y2="13" />
      <line x1="9" y1="17" x2="13" y2="17" />
    </svg>
  </div>
  <p>{message}</p>
  {#if actionLabel && onAction}
    <button type="button" class="ghost" style="margin-top: var(--space-4)" onclick={onAction}>
      {actionLabel}
    </button>
  {/if}
</div>
