<script>
  import { link, location } from "svelte-spa-router";
  import { currentUser } from "../stores/auth.js";
  import { LayoutDashboard, BarChart3, FolderGit2, GitPullRequest, Users, Settings, ClipboardList } from "lucide-svelte";
  import BrandMark from "./BrandMark.svelte";
  import UserAvatar from "./UserAvatar.svelte";

  const navItems = [
    { path: "/app/dashboard", label: "Dashboard", icon: LayoutDashboard },
    { path: "/app/stats", label: "Stats", icon: BarChart3 },
    { path: "/app/repos", label: "Repositories", icon: FolderGit2 },
    { path: "/app/reviews", label: "Reviews", icon: GitPullRequest },
    { path: "/app/team", label: "Team", icon: Users },
    { path: "/app/settings", label: "Settings", icon: Settings },
    { path: "/app/audit", label: "Audit log", icon: ClipboardList },
  ];

  /** Longest matching nav prefix wins (e.g. /app/repos/123 → Repositories). */
  function isActive(path) {
    const loc = $location || "";
    const matches = navItems.filter(
      (item) => loc === item.path || loc.startsWith(`${item.path}/`)
    );
    if (matches.length === 0) return false;
    const best = matches.reduce((a, b) => (a.path.length >= b.path.length ? a : b));
    return best.path === path;
  }
</script>

<aside class="app-sidebar">
  <div class="sidebar-logo">
    <BrandMark size={28} />
    <span>Codasaurus</span>
  </div>
  <nav aria-label="Primary">
    <ul class="sidebar-nav">
      {#each navItems as item}
        <li>
          <a
            href={item.path}
            use:link
            class:active={isActive(item.path)}
            aria-current={isActive(item.path) ? "page" : undefined}
          >
            <svelte:component this={item.icon} size={16} strokeWidth={1.8} />
            <span>{item.label}</span>
          </a>
        </li>
      {/each}
    </ul>
  </nav>
  <div class="sidebar-footer">
    {#if $currentUser}
      <div class="sidebar-user">
        <UserAvatar email={$currentUser.email} size={28} />
        <span class="sidebar-user-email">{$currentUser.email}</span>
      </div>
    {/if}
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
    letter-spacing: -0.02em;
    border-bottom: 1px solid var(--border);
  }

  .sidebar-footer {
    margin-top: auto;
    padding: 16px 20px;
    border-top: 1px solid var(--border);
  }

  .sidebar-user {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }

  .sidebar-user-email {
    font-size: 12px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
