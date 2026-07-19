<script>
  import { push } from "svelte-spa-router";
  import { api } from "../../stores/api.js";

  let email = $state("");
  let password = $state("");
  let confirmPassword = $state("");
  let showPassword = $state(false);
  let showConfirm = $state(false);
  let creating = $state(false);
  let error = $state("");

  async function handleCreate() {
    error = "";
    if (password.length < 8) {
      error = "Password must be at least 8 characters.";
      return;
    }
    if (password !== confirmPassword) {
      error = "Passwords do not match.";
      return;
    }
    creating = true;
    try {
      await api.post("/api/setup/admin", { email, password });
      push("/setup/complete");
    } catch (err) {
      error = err.message || "Failed to create admin";
    } finally {
      creating = false;
    }
  }
</script>

<div class="wizard-card">
  <div class="step-indicator">
    <span class="step-dot completed"></span>
    <span class="step-dot completed"></span>
    <span class="step-dot completed"></span>
    <span class="step-dot active"></span>
  </div>
  <p class="wizard-step-label">Step 4 of 4 — Admin Account</p>

  {#if error}
    <div class="error-state" style="margin-bottom:16px">{error}</div>
  {/if}

  <div class="form-group">
    <label for="email">Email</label>
    <input id="email" type="email" bind:value={email} placeholder="admin@example.com" />
  </div>
  <div class="form-group">
    <label for="password">Password</label>
    <div class="pw-field">
      <input id="password" type={showPassword ? "text" : "password"} bind:value={password} />
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
    <div style="font-size:12px;color:var(--text-muted);margin-top:4px">
      At least 8 characters
      {#if password.length > 0}
        <span style="color:{password.length >= 8 ? 'var(--success)' : 'var(--error)'}">
          — {password.length >= 8 ? "Strong enough" : `${password.length}/8`}
        </span>
      {/if}
    </div>
  </div>
  <div class="form-group">
    <label for="confirm">Confirm Password</label>
    <div class="pw-field">
      <input id="confirm" type={showConfirm ? "text" : "password"} bind:value={confirmPassword} />
      <button type="button" class="pw-toggle" onclick={() => (showConfirm = !showConfirm)} aria-label={showConfirm ? "Hide password" : "Show password"}>
        {#if showConfirm}
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

  <div class="wizard-actions">
    <button onclick={() => push("/setup/github")}>Back</button>
    <button class="primary" onclick={handleCreate} disabled={creating || !email || !password}>
      {creating ? "Creating…" : "Create Admin"}
    </button>
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
