<script>
  let { severity = "info" } = $props();

  const label = {
    blocking: "blocking",
    warning: "warning",
    info: "info",
  };
</script>

<span class="badge severity-{severity}">{label[severity] ?? severity}</span>
