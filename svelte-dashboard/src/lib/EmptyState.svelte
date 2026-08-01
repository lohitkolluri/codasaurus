<script>
  let { message = "No data found.", actionLabel = "", onAction = null } = $props();
</script>

<div class="empty-state" role="status">
  <p>{message}</p>
  {#if actionLabel && onAction}
    <button type="button" class="btn-secondary" style="margin-top:12px" onclick={onAction}>
      {actionLabel}
    </button>
  {/if}
</div>
