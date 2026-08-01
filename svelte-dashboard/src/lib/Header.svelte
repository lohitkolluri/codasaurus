<script>
  import { currentUser, logout, roleLabel } from "../stores/auth.js";
  import { push } from "svelte-spa-router";
  import { Sun, Moon, LogOut } from "lucide-svelte";

  let { title = "" } = $props();
  let theme = $state(typeof localStorage !== "undefined" ? (localStorage.getItem("codasaurus-theme") ?? "dark") : "dark");

  $effect(() => document.documentElement.setAttribute("data-theme", theme));

  function toggleTheme() {
    theme = theme === "light" ? "dark" : "light";
    localStorage.setItem("codasaurus-theme", theme);
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
    {#if $currentUser}
      <span class="header-user">
        {$currentUser.email}
        {#if $currentUser.role}
          <span class="role-badge" class:bootstrap={!!$currentUser.is_bootstrap}>
            {roleLabel($currentUser.role, $currentUser.is_bootstrap)}
          </span>
        {/if}
      </span>
    {/if}
    <button class="header-btn icon-btn" aria-label="Toggle theme" title="Toggle theme" onclick={toggleTheme}>
      {#if theme === "light"}<Moon size={16} />{:else}<Sun size={16} />{/if}
    </button>
    <button class="header-btn icon-btn" aria-label="Log out" title="Log out" onclick={handleLogout}><LogOut size={16} /></button>
  </div>
</header>
