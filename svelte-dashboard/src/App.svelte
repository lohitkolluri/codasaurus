<script>
  import Router, { push, location } from "svelte-spa-router";
  import { checkSession, currentUser, authLoading } from "./stores/auth.js";
  import { onMount } from "svelte";
  import LoadingSpinner from "./lib/LoadingSpinner.svelte";

  // Pages
  import Login from "./pages/Login.svelte";
  import NotFound from "./pages/NotFound.svelte";

  // Wizard
  import SetupWizard from "./pages/wizard/SetupWizard.svelte";
  import SetupDatabase from "./pages/wizard/SetupDatabase.svelte";
  import SetupLLM from "./pages/wizard/SetupLLM.svelte";
  import SetupGitHub from "./pages/wizard/SetupGitHub.svelte";
  import SetupGitHubCallback from "./pages/wizard/SetupGitHubCallback.svelte";
  import SetupAdmin from "./pages/wizard/SetupAdmin.svelte";
  import SetupComplete from "./pages/wizard/SetupComplete.svelte";

  // App pages
  import Dashboard from "./pages/app/Dashboard.svelte";
  import Repos from "./pages/app/Repos.svelte";
  import RepoDetail from "./pages/app/RepoDetail.svelte";
  import Reviews from "./pages/app/Reviews.svelte";
  import ReviewDetail from "./pages/app/ReviewDetail.svelte";
  import Settings from "./pages/app/Settings.svelte";
  import GitHubSettings from "./pages/app/GitHubSettings.svelte";
  import AuditLog from "./pages/app/AuditLog.svelte";

  let loading = $state(true);
  let initialCheckDone = $state(false);

  onMount(async () => {
    await checkSession();
    loading = false;
    initialCheckDone = true;
  });

  // Conditionally redirect based on auth state
  $effect(() => {
    if (!initialCheckDone || $authLoading) return;
    const loc = $location;
    const user = $currentUser;

    if (loc.startsWith("/app") && !user) {
      push("/login");
    }
  });

  const routes = {
    "/": SetupWizard,
    "/setup": SetupWizard,
    "/setup/database": SetupDatabase,
    "/setup/llm": SetupLLM,
    "/setup/github": SetupGitHub,
    "/setup/github/callback": SetupGitHubCallback,
    "/setup/admin": SetupAdmin,
    "/setup/complete": SetupComplete,
    "/login": Login,
    "/app/dashboard": Dashboard,
    "/app/repos": Repos,
    "/app/repos/:id": RepoDetail,
    "/app/reviews": Reviews,
    "/app/reviews/:id": ReviewDetail,
    "/app/settings": Settings,
    "/app/settings/github": GitHubSettings,
    "/app/audit": AuditLog,
    "*": NotFound,
  };
</script>

<LoadingSpinner loading={loading || $authLoading} />

{#if !loading}
  <Router {routes} />
{/if}
