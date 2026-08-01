<script>
  import Sidebar from "./Sidebar.svelte";
  import Header from "./Header.svelte";

  let { title = "", children } = $props();
</script>

<div class="app-layout">
  <Sidebar />
  <div class="app-main">
    <Header {title} />
    <div class="app-content">
      {@render children()}
    </div>
  </div>
</div>
