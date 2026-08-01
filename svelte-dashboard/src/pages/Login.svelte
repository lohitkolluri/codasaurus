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

<div class="login-page">
  <div class="login-card">
    <div class="login-brand">
      <BrandMark size={40} />
      <h1>Codasaurus</h1>
    </div>
    <p class="subtitle">Self-hosted PR review agent</p>

    {#if error}
      <div class="login-error" role="alert">{error}</div>
    {/if}

    {#if oidcEnabled}
      <button type="button" class="primary" style="width:100%" onclick={ssoLogin}>
        Sign in with SSO
      </button>
      <div class="login-divider">or email</div>
    {/if}

    <form onsubmit={handleSubmit}>
      <div class="form-group">
        <label for="email">Email</label>
        <input id="email" type="email" bind:value={email} required placeholder="you@example.com" autocomplete="username" />
      </div>
      <div class="form-group">
        <label for="password">Password</label>
        <input id="password" type="password" bind:value={password} required autocomplete="current-password" />
      </div>
      <button type="submit" class="primary" style="width:100%;margin-top:8px" disabled={submitting}>
        {submitting ? "Signing in…" : "Sign In"}
      </button>
    </form>
    <p class="login-invite-hint">Have an invite link? Open it to create your account.</p>
  </div>
</div>
