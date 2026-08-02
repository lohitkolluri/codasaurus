<script>
  let { message = "An error occurred." } = $props();
</script>

<div class="error-state" role="alert">{message}</div>
