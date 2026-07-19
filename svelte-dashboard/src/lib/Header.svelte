<script>
  import { currentUser, logout } from "../stores/auth.js";
  import { push } from "svelte-spa-router";

  let { title = "" } = $props();
  let theme = $state("light");

  function toggleTheme() {
    theme = theme === "light" ? "dark" : "light";
    document.documentElement.setAttribute("data-theme", theme);
  }

  async function handleLogout() {
    await logout();
    push("/login");
  }
</script>

<header class="top-header">
  <div class="top-header-left">
      <span class="top-header-title">{title}</span>
  </div>
  <div class="top-header-right">
    <span class="header-user">{($currentUser?.email) ?? ""}</span>
    <button class="header-btn" onclick={toggleTheme}>
      {theme === "light" ? "Dark" : "Light"}
    </button>
    <button class="header-btn" onclick={handleLogout}>Logout</button>
  </div>
</header>
