<script>
  import { link, location } from "svelte-spa-router";
  import { currentUser } from "../stores/auth.js";
  import { LayoutDashboard, FolderGit2, GitPullRequest, Settings, ClipboardList, KeyRound } from "lucide-svelte";

  const navItems = [
    { path: "/app/dashboard", label: "Dashboard", icon: LayoutDashboard },
    { path: "/app/repos", label: "Repositories", icon: FolderGit2 },
    { path: "/app/reviews", label: "Reviews", icon: GitPullRequest },
    { path: "/app/settings", label: "Settings", icon: Settings },
    { path: "/app/settings/github", label: "GitHub App", icon: KeyRound },
    { path: "/app/audit", label: "Audit log", icon: ClipboardList },
  ];
</script>

<aside class="app-sidebar">
  <div class="sidebar-logo">
    <svg width="28" height="28" viewBox="0 0 32 32" fill="none">
      <!-- Laptop screen (background) -->
      <rect x="2" y="4" width="28" height="18" rx="2" fill="currentColor" opacity="0.12" stroke="currentColor" stroke-width="1" />
      <rect x="4" y="5.5" width="24" height="14" rx="1.5" fill="currentColor" opacity="0.06" />
      <!-- Code lines -->
      <rect x="6" y="8" width="8" height="1" rx="0.5" fill="var(--accent-soft)" opacity="0.3" />
      <rect x="6" y="10.5" width="5" height="1" rx="0.5" fill="currentColor" opacity="0.2" />
      <rect x="6" y="13" width="10" height="1" rx="0.5" fill="currentColor" opacity="0.15" />
      <!-- Keyboard base -->
      <rect x="2" y="23" width="28" height="3" rx="1" fill="var(--accent-soft)" />
      <rect x="2" y="25.5" width="28" height="1.5" rx="0.7" fill="currentColor" opacity="0.2" />
      <rect x="5" y="22.5" width="22" height="2" rx="0.8" fill="currentColor" opacity="0.1" />
      <!-- T-Rex (original SVG path, scaled + positioned) -->
      <g transform="translate(3, 0) scale(0.95)">
        <path d="m20.546,9c1.904,0,3.454-1.55,3.454-3.454,0-2.704-2.02-5.08-4.696-5.463-2.725-.389-5.475.608-7.362,2.664-2.253,2.455-5.349,6.4-7.3,11.024-.096.23-.487.266-.812.199-.679-.138-1.829-.859-1.829-2.691v-2.278c0-.553-.448-1-1-1s-1,.447-1,1v3.51c0,4.826,3.578,8.833,8.324,9.321.543.056,1.104.097,1.676.128v1.041c0,.553.448,1,1,1s1-.447,1-1v-1.003c.694-.042,1.368-.188,2-.444v.447c0,1.103.897,2,2,2h2c.553,0,1-.447,1-1s-.447-1-1-1h-2v-2c-.002-.252-.106-6.254-6.125-7.008-.548-.068-.937-.568-.868-1.116.068-.548.565-.933,1.117-.868,4.73.593,6.814,3.872,7.545,6.63.214-.644.33-1.324.33-2.024v-2.19c.354.236.77.604,1.2,1.177.196.263.496.4.801.4.209,0,.419-.065.599-.2.442-.331.532-.958.2-1.399-1.018-1.358-2.059-1.937-2.8-2.194v-2.207h2.546Z" fill="currentColor" />
      </g>
      <!-- Legs (standing on keyboard) -->
      <rect x="9" y="18" width="2.5" height="5" rx="1" fill="currentColor" />
      <rect x="14" y="18" width="2.5" height="5" rx="1" fill="currentColor" />
      <!-- Feet -->
      <rect x="7.5" y="22" width="5" height="1.5" rx="0.7" fill="currentColor" />
      <rect x="12.5" y="22" width="5" height="1.5" rx="0.7" fill="currentColor" />
      <!-- Eye -->
      <circle cx="23.5" cy="6.5" r="1.2" fill="var(--bg-primary)" />
      <circle cx="23.8" cy="6.5" r="0.6" fill="var(--accent-soft)" />
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
            <svelte:component this={item.icon} size={16} strokeWidth={1.8} />
            <span>{item.label}</span>
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
