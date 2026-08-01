<script>
  import { onMount } from "svelte";
  import { push, link } from "svelte-spa-router";
  import { currentUser, checkSession } from "../stores/auth.js";
  import BrandMark from "../lib/BrandMark.svelte";

  let { params = {} } = $props();
  let token = $derived(params.token || "");

  let loading = $state(true);
  let error = $state("");
  let invite = $state(null);
  let email = $state("");
  let password = $state("");
  let password2 = $state("");
  let submitting = $state(false);

  onMount(async () => {
    if (!token) {
      error = "Missing invite token";
      loading = false;
      return;
    }
    try {
      const res = await fetch(`/api/auth/invite/${encodeURIComponent(token)}`, {
        credentials: "same-origin",
      });
      const data = await res.json().catch(() => ({}));
      if (!res.ok) {
        error = data.error || "Invite not found or expired";
        return;
      }
      invite = data;
      if (data.email) email = data.email;
    } catch (err) {
      error = err.message || "Failed to load invite";
    } finally {
      loading = false;
    }
  });

  async function handleSubmit(e) {
    e.preventDefault();
    error = "";
    if (password.length < 8) {
      error = "Password must be at least 8 characters";
      return;
    }
    if (password !== password2) {
      error = "Passwords do not match";
      return;
    }
    submitting = true;
    try {
      const res = await fetch(`/api/auth/invite/${encodeURIComponent(token)}/accept`, {
        method: "POST",
        credentials: "same-origin",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ email: email || undefined, password }),
      });
      const data = await res.json().catch(() => ({}));
      if (!res.ok) {
        error = data.error || "Could not accept invite";
        return;
      }
      await checkSession();
      push("/app/dashboard");
    } catch (err) {
      error = err.message || "Could not accept invite";
    } finally {
      submitting = false;
    }
  }
</script>

<div class="auth-shell">
  <aside class="auth-stage" aria-hidden="true">
    <div class="auth-stage-grid"></div>
    <div class="auth-stage-glow"></div>
    <div class="auth-stage-inner">
      <BrandMark size={56} />
      <p class="auth-stage-kicker">Codasaurus</p>
      <h2 class="auth-stage-title">You're invited to the team.</h2>
      <p class="auth-stage-copy">
        Set a password to join this Codasaurus instance. Owners manage access with invite links — no email server required.
      </p>
    </div>
  </aside>

  <main class="auth-panel">
    <div class="auth-panel-inner">
      <div class="auth-mobile-brand">
        <BrandMark size={36} />
        <span>Codasaurus</span>
      </div>

      <h1 class="auth-heading">Accept invite</h1>

      {#if loading}
        <p class="auth-lead">Checking invite…</p>
      {:else if !invite}
        <div class="login-error" role="alert">{error || "Invalid invite"}</div>
        <p class="login-invite-hint"><a href="#/login" use:link>Back to sign in</a></p>
      {:else}
        <p class="auth-lead">
          Joining as <strong>{invite.role}</strong>
          {#if invite.email} · {invite.email}{/if}
        </p>
        {#if error}
          <div class="login-error" role="alert">{error}</div>
        {/if}
        <form class="auth-form" onsubmit={handleSubmit}>
          <div class="form-group">
            <label for="invite-email">Email</label>
            <input
              id="invite-email"
              type="email"
              bind:value={email}
              required
              readonly={!!invite.email_locked}
              placeholder="you@company.com"
              autocomplete="username"
            />
          </div>
          <div class="form-group">
            <label for="invite-password">Password</label>
            <input
              id="invite-password"
              type="password"
              bind:value={password}
              required
              minlength="8"
              autocomplete="new-password"
            />
          </div>
          <div class="form-group">
            <label for="invite-password2">Confirm password</label>
            <input
              id="invite-password2"
              type="password"
              bind:value={password2}
              required
              minlength="8"
              autocomplete="new-password"
            />
          </div>
          <button type="submit" class="primary auth-submit" disabled={submitting}>
            {submitting ? "Creating account…" : "Create account"}
          </button>
        </form>
        {#if $currentUser}
          <p class="login-invite-hint">Already signed in as {$currentUser.email}.</p>
        {/if}
      {/if}
    </div>
  </main>
</div>
