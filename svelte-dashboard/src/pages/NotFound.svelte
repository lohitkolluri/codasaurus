<script>
  import { push } from "svelte-spa-router";
</script>

<div class="center-page">
  <h1>404</h1>
  <p>Page not found</p>
  <button class="link-style" onclick={() => push("/")}>Go home</button>
</div>
