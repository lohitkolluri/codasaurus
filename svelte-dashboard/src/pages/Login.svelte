<script>
  import { push } from "svelte-spa-router";
  import { login } from "../stores/auth.js";

  let email = $state("");
  let password = $state("");
  let showPassword = $state(false);
  let error = $state("");
  let submitting = $state(false);

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
</script>

<div class="login-page">
  <div class="login-card">
    <h1>Codasaurus</h1>
    <p class="subtitle">AI Code Review Platform</p>

    {#if error}
      <div class="login-error">{error}</div>
    {/if}

    <form onsubmit={handleSubmit}>
      <div class="form-group">
        <label for="email">Email</label>
        <input id="email" type="email" bind:value={email} required placeholder="you@example.com" />
      </div>
      <div class="form-group">
        <label for="password">Password</label>
        <div class="pw-field">
          <input id="password" type={showPassword ? "text" : "password"} bind:value={password} required />
          <button type="button" class="pw-toggle" onclick={() => (showPassword = !showPassword)} aria-label={showPassword ? "Hide password" : "Show password"}>
            {#if showPassword}
              <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94"/>
                <path d="M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19"/>
                <line x1="1" y1="1" x2="23" y2="23"/>
                <path d="M14.12 14.12a3 3 0 1 1-4.24-4.24"/>
              </svg>
            {:else}
              <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/>
                <circle cx="12" cy="12" r="3"/>
              </svg>
            {/if}
          </button>
        </div>
      </div>
      <button type="submit" class="primary" style="width:100%;margin-top:8px" disabled={submitting}>
        {submitting ? "Signing in…" : "Sign In"}
      </button>
    </form>
  </div>
</div>

<style>
  .pw-field {
    position: relative;
    display: flex;
    align-items: center;
  }
  .pw-field input {
    width: 100%;
    padding-right: 40px;
  }
  .pw-toggle {
    position: absolute;
    right: 4px;
    top: 50%;
    transform: translateY(-50%);
    background: none;
    border: none;
    cursor: pointer;
    padding: 6px;
    color: var(--text-muted);
    display: flex;
    align-items: center;
    border-radius: 4px;
    line-height: 0;
  }
  .pw-toggle:hover {
    color: var(--text-primary);
    background: var(--bg-secondary);
  }
</style>
