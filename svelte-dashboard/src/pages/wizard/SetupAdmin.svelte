<script>
  import { push } from "svelte-spa-router";
  import { api } from "../../stores/api.js";

  let email = $state("");
  let password = $state("");
  let confirmPassword = $state("");
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
    <input id="password" type="password" bind:value={password} />
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
    <input id="confirm" type="password" bind:value={confirmPassword} />
  </div>

  <div class="wizard-actions">
    <button onclick={() => push("/setup/github")}>Back</button>
    <button class="primary" onclick={handleCreate} disabled={creating || !email || !password}>
      {creating ? "Creating…" : "Create Admin"}
    </button>
  </div>
</div>
