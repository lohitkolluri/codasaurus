<script>
  import { onMount } from "svelte";
  import { push } from "svelte-spa-router";
  import { api } from "../../stores/api.js";
  import WizardShell from "../../lib/WizardShell.svelte";

  let email = $state("");
  let password = $state("");
  let confirmPassword = $state("");
  let creating = $state(false);
  let error = $state("");
  let status = $state(null);

  const lengthOk = $derived(password.length >= 10);
  const matchOk = $derived(password.length > 0 && password === confirmPassword);
  const canSubmit = $derived(!!email && lengthOk && matchOk && !creating);

  onMount(async () => {
    try {
      status = await api.get("/api/setup/status");
      if (status?.admin) {
        push("/setup/complete");
      }
    } catch {
      /* ignore */
    }
  });

  async function handleCreate() {
    error = "";
    if (!lengthOk) {
      error = "Password must be at least 10 characters.";
      return;
    }
    if (!matchOk) {
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

<WizardShell
  current="admin"
  {status}
  title="Create your admin login"
  subtitle="This is the account that unlocks the dashboard. Store the password somewhere safe. There's no email reset yet."
>
  {#if error}
    <div class="error-box">{error}</div>
  {/if}

  <div class="form-group">
    <label for="email">Email</label>
    <input id="email" type="email" bind:value={email} placeholder="you@company.com" autocomplete="username" />
  </div>
  <div class="form-group">
    <label for="password">Password</label>
    <input id="password" type="password" bind:value={password} autocomplete="new-password" />
    <div class="wizard-hint">
      At least 8 characters
      {#if password.length > 0}
        <span style="color:{lengthOk ? 'var(--success)' : 'var(--error)'}">
          {lengthOk ? "meets minimum" : `${password.length}/10`}
        </span>
      {/if}
    </div>
  </div>
  <div class="form-group">
    <label for="confirm">Confirm password</label>
    <input id="confirm" type="password" bind:value={confirmPassword} autocomplete="new-password" />
    {#if confirmPassword.length > 0 && !matchOk}
      <div class="wizard-hint" style="color:var(--error)">Passwords do not match</div>
    {/if}
  </div>

  <div class="wizard-actions">
    <button onclick={() => push("/setup/github")}>Back</button>
    <button class="primary" onclick={handleCreate} disabled={!canSubmit}>
      {creating ? "Creating…" : "Create admin & finish"}
    </button>
  </div>
</WizardShell>
