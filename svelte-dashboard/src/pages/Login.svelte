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
        <div class="password-wrapper">
          <input id="password" type={showPassword ? "text" : "password"} bind:value={password} required />
          <button type="button" class="toggle-pw" onclick={() => (showPassword = !showPassword)}>
            {showPassword ? "🙈" : "👁️"}
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
  .password-wrapper {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .password-wrapper input {
    flex: 1;
  }
  .toggle-pw {
    padding: 6px 10px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 6px;
    cursor: pointer;
    font-size: 16px;
    line-height: 1;
  }
  .toggle-pw:hover {
    background: var(--bg-hover);
  }
</style>
