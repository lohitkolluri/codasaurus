<script>
  import Router, { push, location } from "svelte-spa-router";
  import { checkSession, currentUser, authLoading } from "./stores/auth.js";
  import { onMount } from "svelte";
  import LoadingSpinner from "./lib/LoadingSpinner.svelte";

  // Pages
  import Login from "./pages/Login.svelte";
  import Landing from "./pages/Landing.svelte";
  import NotFound from "./pages/NotFound.svelte";

  // Wizard
  import SetupWizard from "./pages/wizard/SetupWizard.svelte";
  import SetupDatabase from "./pages/wizard/SetupDatabase.svelte";
  import SetupLLM from "./pages/wizard/SetupLLM.svelte";
  import SetupGitHub from "./pages/wizard/SetupGitHub.svelte";
  import SetupAdmin from "./pages/wizard/SetupAdmin.svelte";
  import SetupComplete from "./pages/wizard/SetupComplete.svelte";

  // App pages
  import Dashboard from "./pages/app/Dashboard.svelte";
  import Stats from "./pages/app/Stats.svelte";
  import Repos from "./pages/app/Repos.svelte";
  import RepoDetail from "./pages/app/RepoDetail.svelte";
  import Reviews from "./pages/app/Reviews.svelte";
  import ReviewDetail from "./pages/app/ReviewDetail.svelte";
  import Settings from "./pages/app/Settings.svelte";
  import AuditLog from "./pages/app/AuditLog.svelte";
  import Team from "./pages/app/Team.svelte";
  import InviteAccept from "./pages/InviteAccept.svelte";

  let loading = $state(true);
  let initialCheckDone = $state(false);

  // Conditionally redirect based on auth + setup state
  $effect(() => {
    if (!initialCheckDone || $authLoading) return;
    const loc = $location;
    const user = $currentUser;

    if (loc.startsWith("/app") && !user) {
      push("/login");
    }
  });

  onMount(async () => {
    await checkSession();
    loading = false;
    initialCheckDone = true;

    // If setup is already complete, don't trap users on the wizard hub.
    try {
      const loc = window.location.hash.replace(/^#/, "") || "/";
      if (loc === "/setup") {
        const status = await fetch("/api/setup/status", { credentials: "same-origin" }).then((r) =>
          r.ok ? r.json() : null,
        );
        if (status?.complete) {
          push($currentUser ? "/app/dashboard" : "/");
        }
      }
    } catch {
      /* stay on current route */
    }
  });

  const routes = {
    "/": Landing,
    "/setup": SetupWizard,
    "/setup/database": SetupDatabase,
    "/setup/llm": SetupLLM,
    "/setup/github": SetupGitHub,
    "/setup/admin": SetupAdmin,
    "/setup/complete": SetupComplete,
    "/login": Login,
    "/invite/:token": InviteAccept,
    "/app/dashboard": Dashboard,
    "/app/stats": Stats,
    "/app/repos": Repos,
    "/app/repos/:id": RepoDetail,
    "/app/reviews": Reviews,
    "/app/reviews/:id": ReviewDetail,
    "/app/settings": Settings,
    "/app/settings/github": Settings,
    "/app/settings/system": Settings,
    "/app/settings/runtime": Settings,
    "/app/settings/oidc": Settings,
    "/app/settings/auth": Settings,
    "/app/settings/learning": Settings,
    "/app/team": Team,
    "/app/audit": AuditLog,
    "*": NotFound,
  };
</script>

<LoadingSpinner loading={loading || $authLoading} />

{#if !loading}
  <Router {routes} />
{/if}
