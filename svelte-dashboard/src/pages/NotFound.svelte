<script>
  import { push } from "svelte-spa-router";
  import BrandMark from "../lib/BrandMark.svelte";
</script>

<div class="center-page">
  <BrandMark size={32} />
  <p class="eyebrow">404</p>
  <div class="scan-rule" style="margin-inline:auto"></div>
  <h1>Page not found</h1>
  <p>This trail went cold. The page you're looking for doesn't exist.</p>
  <button class="primary" onclick={() => push("/")}>Go home</button>
</div>
