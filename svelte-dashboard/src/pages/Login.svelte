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

<div class="auth-matte">
  <!-- Full-bleed dark stage + bold wordmark as atmosphere -->
  <div class="auth-matte-wordmark" aria-hidden="true">CODASAURUS</div>

  <!-- Elevated opaque card: welcome headline + clear form -->
  <div class="auth-matte-panel">
    <header class="auth-matte-hero">
      <BrandMark size={32} />
      <div class="auth-matte-titles">
        <h1>Welcome back</h1>
        <p class="auth-matte-lead">Sign in to your Codasaurus instance</p>
      </div>
    </header>

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
        {submitting ? "Signing in…" : "Log in"}
      </button>
    </form>

    <p class="login-invite-hint">
      <a href="#/" use:link>Home</a>
      <span aria-hidden="true"> · </span>
      Need an invite? Ask your instance owner
    </p>
  </div>
</div>
