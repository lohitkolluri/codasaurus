<script>
  import { onMount } from "svelte";
  import { push } from "svelte-spa-router";
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

<div class="auth-shell">
  <aside class="auth-stage" aria-hidden="true">
    <div class="auth-stage-grid"></div>
    <div class="auth-stage-glow"></div>
    <div class="auth-stage-inner">
      <BrandMark size={56} />
      <p class="auth-stage-kicker">Codasaurus</p>
      <h2 class="auth-stage-title">Reviews that actually read the diff.</h2>
      <p class="auth-stage-copy">
        Self-hosted GitHub App reviews. Tier-1 detectors first, optional BYOK LLM when you want it.
      </p>
      <ul class="auth-stage-list">
        <li>Secrets, deps, and risky patterns</li>
        <li>Walkthroughs and slash commands</li>
        <li>Runs on your Postgres, your keys</li>
      </ul>
    </div>
  </aside>

  <main class="auth-panel">
    <div class="auth-panel-inner">
      <div class="auth-mobile-brand">
        <BrandMark size={36} />
        <span>Codasaurus</span>
      </div>

      <h1 class="auth-heading">Sign in</h1>
      <p class="auth-lead">Access your review dashboard.</p>

      {#if error}
        <div class="login-error" role="alert">{error}</div>
      {/if}

      {#if oidcEnabled}
        <button type="button" class="auth-sso" onclick={ssoLogin}>
          Continue with SSO
        </button>
        <div class="login-divider"><span>or email</span></div>
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
        Have an invite link? Open it to create your account.
      </p>
    </div>
  </main>
</div>
