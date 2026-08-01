<script>
  import { onMount } from "svelte";
  import { push, link } from "svelte-spa-router";
  import { login } from "../stores/auth.js";
  import BrandMark from "../lib/BrandMark.svelte";

  let email = $state("");
  let password = $state("");
  let error = $state("");
  let submitting = $state(false);
  let oidcEnabled = $state(false);

  onMount(async () => {
    try {
      const r = await fetch("/api/auth/oidc/status");
      if (r.ok) {
        const j = await r.json();
        oidcEnabled = !!j.enabled;
      }
    } catch {
      /* ignore */
    }
  });

  async function handleSubmit(e) {
    e.preventDefault();
    error = "";
    submitting = true;
    try {
      await login(email, password);
      push("/app/dashboard");
    } catch (err) {
      error = err.message || "Invalid email or password.";
    } finally {
      submitting = false;
    }
  }

  function ssoLogin() {
    window.location.href = "/api/auth/oidc/login";
  }
</script>

<div class="auth-minimal">
  <div class="auth-minimal-bg" aria-hidden="true"></div>
  <div class="auth-minimal-inner">
    <header class="auth-minimal-brand">
      <BrandMark size={40} />
      <h1>Codasaurus</h1>
    </header>

    <p class="auth-minimal-lead">Sign in to your instance</p>

    {#if error}
      <div class="login-error" role="alert">{error}</div>
    {/if}

    {#if oidcEnabled}
      <button type="button" class="auth-sso" onclick={ssoLogin}>Continue with SSO</button>
      <div class="login-divider"><span>or</span></div>
    {/if}

    <form class="auth-form" onsubmit={handleSubmit}>
      <div class="form-group">
        <label for="email">Email</label>
        <input
          id="email"
          type="email"
          bind:value={email}
          required
          placeholder="you@company.com"
          autocomplete="username"
        />
      </div>
      <div class="form-group">
        <label for="password">Password</label>
        <input
          id="password"
          type="password"
          bind:value={password}
          required
          autocomplete="current-password"
        />
      </div>
      <button type="submit" class="primary auth-submit" disabled={submitting}>
        {submitting ? "Signing in…" : "Sign in"}
      </button>
    </form>

    <p class="login-invite-hint">
      <a href="#/" use:link>Home</a>
      <span aria-hidden="true"> · </span>
      Invite link opens a join page
    </p>
  </div>
</div>
