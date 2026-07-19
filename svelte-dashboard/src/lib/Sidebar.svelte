<script>
  import { link, location } from "svelte-spa-router";
  import { currentUser } from "../stores/auth.js";

  const navItems = [
    { path: "/app/dashboard", label: "Dashboard" },
    { path: "/app/repos", label: "Repos" },
    { path: "/app/reviews", label: "Reviews" },
    { path: "/app/settings", label: "Settings" },
    { path: "/app/audit", label: "Audit Log" },
  ];
</script>

<aside class="app-sidebar">
  <div class="sidebar-logo">
    <svg width="20" height="20" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="2">
      <path d="M10 2L3 6v8l7 4 7-4V6l-7-4z"/>
      <path d="M10 10l7-4"/>
      <path d="M10 10v8"/>
      <path d="M3 6l7 4"/>
    </svg>
    Codasaurus
  </div>
  <nav>
    <ul class="sidebar-nav">
      {#each navItems as item}
        <li>
          <a
            href={item.path}
            use:link
            class:active={$location?.startsWith(item.path)}
          >
            {item.label}
          </a>
        </li>
      {/each}
    </ul>
  </nav>
  <div class="sidebar-footer">
    <div class="sidebar-user">{($currentUser?.email) ?? ""}</div>
  </div>
</aside>

<style>
  .sidebar-logo {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 20px;
    font-size: 16px;
    font-weight: 700;
    border-bottom: 1px solid var(--border);
  }

  .sidebar-footer {
    margin-top: auto;
    padding: 16px 20px;
    border-top: 1px solid var(--border);
  }

  .sidebar-user {
    font-size: 12px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
