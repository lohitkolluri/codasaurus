<script>
  import { onMount } from "svelte";
  import { location } from "svelte-spa-router";
  import { api } from "../../stores/api.js";
  import { currentUser, isOwner, isMaintainer, roleLabel } from "../../stores/auth.js";
  import { formatLabel } from "../../lib/utils.js";
  import AppShell from "../../lib/AppShell.svelte";
  import LoadingSpinner from "../../lib/LoadingSpinner.svelte";
  import ErrorState from "../../lib/ErrorState.svelte";

  const TABS = [
    { id: "llm", label: "LLM" },
    { id: "detectors", label: "Detectors" },
    { id: "policy", label: "Policy" },
    { id: "runtime", label: "Runtime" },
    { id: "auth", label: "Auth / SSO" },
    { id: "integrations", label: "Integrations" },
    { id: "github", label: "GitHub" },
    { id: "account", label: "Account" },
    { id: "learning", label: "Learning" },
  ];

  let canEditSettings = $derived($isOwner);

  let settings = $state({});
  let loading = $state(true);
  let error = $state("");
  let activeTab = $state("llm");

  let llmProvider = $state("openrouter");
  let llmKey = $state("");
  let llmModel = $state("");
  let llmModelCheap = $state("");
  let llmBaseUrl = $state("");
  let llmDailyBudget = $state("");
  let llmSaving = $state(false);
  let llmMsg = $state("");

  let detectorToggles = $state({});
  let detectorSaving = $state(false);
  let detectorMsg = $state("");

  let defaultSeverity = $state("warning");
  let severitySaving = $state(false);
  let severityMsg = $state("");

  let models = $state([]);
  let modelSearch = $state("");
  let modelDropdown = $state(false);
  let cheapModelSearch = $state("");
  let cheapModelDropdown = $state(false);
  let modelFiltered = $derived.by(() => filterModels(modelSearch));
  let cheapModelFiltered = $derived.by(() => filterModels(cheapModelSearch));

  const OPENROUTER_BASE = "https://openrouter.ai/api/v1";

  function filterModels(q) {
    if (!q) return models.slice(0, 20);
    const needle = q.toLowerCase();
    return models
      .filter((m) => m.id.toLowerCase().includes(needle) || m.name.toLowerCase().includes(needle))
      .slice(0, 15);
  }

  const DETECTOR_KEYS = [
    "hallucinated_imports", "phantom_deps", "vulnerabilities", "secrets",
    "over_engineering", "boilerplate", "todo_leaks", "stale_api", "risky_patterns", "graph", "guidelines", "iac",
  ];

  let maxWarnings = $state("20");
  let maxBlocking = $state("0");
  let reviewStrictness = $state("balanced");
  let forbiddenPaths = $state("");
  let autoLabels = $state(true);
  let requestReviewers = $state(true);
  let createCheckRun = $state(true);
  let excludePatterns = $state("");
  let customInstructions = $state("");
  let updatePrDescription = $state(false);
  let allowAutoFix = $state(false);
  let offlineMode = $state(false);
  let policySaving = $state(false);
  let policyMsg = $state("");
  let learnedRules = $state([]);
  let rulesMsg = $state("");

  let github = $state(null);
  let githubMsg = $state("");
  let clearingGithub = $state(false);
  let confirmClearGithub = $state(false);

  let pwCurrent = $state("");
  let pwNew = $state("");
  let pwMsg = $state("");
  let pwSaving = $state(false);

  // Runtime / ops
  let publicUrl = $state("");
  let auditRetentionDays = $state("90");
  let queueWorkers = $state("");
  let maxConcurrentReviews = $state("");
  let hsts = $state(false);
  let metricsToken = $state("");
  let reviewTimeoutSecs = $state("300");
  let maxInlineComments = $state("8");
  let maxReviewerFiles = $state("8");
  let maxCommentBytes = $state("64000");
  let maxLlmDiffChars = $state("8000");
  let autoImproveMaxFiles = $state("40");
  let autoImproveMaxDiff = $state("24000");
  let allowLocalLlm = $state(false);
  let insecureCookies = $state(false);
  let secureCookies = $state(false);
  let runtimeSaving = $state(false);
  let runtimeMsg = $state("");

  // OIDC
  let oidcIssuer = $state("");
  let oidcClientId = $state("");
  let oidcClientSecret = $state("");
  let oidcRedirectUri = $state("");
  let oidcScopes = $state("openid email profile");
  let oidcAllowOpenJoin = $state(false);
  let oidcAllowUnverifiedEmail = $state(false);
  let oidcAllowPublicClient = $state(false);
  let authSaving = $state(false);
  let authMsg = $state("");

  // Integrations
  let jiraBaseUrl = $state("");
  let jiraEmail = $state("");
  let jiraApiToken = $state("");
  let linearApiKey = $state("");
  let integrationsSaving = $state(false);
  let integrationsMsg = $state("");

  const PROVIDER_DEFAULTS = {
    openrouter: { model: "openai/gpt-4o", baseUrl: OPENROUTER_BASE },
    ollama: { model: "llama3", baseUrl: "http://localhost:11434/v1" },
    custom: { model: "", baseUrl: "" },
    disabled: { model: "", baseUrl: "" },
  };

  async function loadModels() {
    try {
      const res = await fetch("https://openrouter.ai/api/v1/models");
      const data = await res.json();
      models = (data.data || []).map((m) => ({ id: m.id, name: m.name || m.id }));
    } catch { /* offline */ }
  }

  function handleProviderChange(val) {
    llmProvider = val;
    const cfg = PROVIDER_DEFAULTS[val];
    if (cfg) {
      llmModel = cfg.model;
      llmBaseUrl = cfg.baseUrl;
      modelSearch = cfg.model;
    }
    if (val === "openrouter") {
      llmBaseUrl = OPENROUTER_BASE;
      if (models.length === 0) loadModels();
    }
  }

  function selectModel(m) {
    llmModel = m.id;
    modelSearch = m.id;
    modelDropdown = false;
  }

  function selectCheapModel(m) {
    llmModelCheap = m.id;
    cheapModelSearch = m.id;
    cheapModelDropdown = false;
  }

  function handleModelKeydown(e) {
    if (e.key === "Escape") { modelDropdown = false; e.target.blur(); }
  }

  function handleCheapModelKeydown(e) {
    if (e.key === "Escape") { cheapModelDropdown = false; e.target.blur(); }
  }

  function handleModelBlur() {
    setTimeout(() => (modelDropdown = false), 150);
  }

  function handleCheapModelBlur() {
    setTimeout(() => (cheapModelDropdown = false), 150);
  }

  function selectTab(id) {
    if (!TABS.some((t) => t.id === id)) return;
    activeTab = id;
  }

  function applyDeepLink() {
    if ($location === "/app/settings/github") {
      activeTab = "github";
    }
  }

  async function changePassword() {
    pwSaving = true;
    pwMsg = "";
    try {
      await api.post("/api/users/me/password", {
        current_password: pwCurrent,
        new_password: pwNew,
      });
      pwMsg = "Password updated";
      pwCurrent = "";
      pwNew = "";
    } catch (err) {
      pwMsg = err.message || "Password change failed";
    } finally {
      pwSaving = false;
    }
  }

  onMount(() => {
    (async () => {
      try {
        const [data, gh] = await Promise.all([
          api.get("/api/settings"),
          api.get("/api/settings/github").catch(() => null),
        ]);
        settings = data;
        github = gh;
        llmProvider = data.llm_provider ?? "openrouter";
        llmKey = data.openrouter_api_key ?? data.llm_api_key ?? "";
        llmModel = data.llm_model ?? "";
        llmModelCheap = data.llm_model_cheap ?? "";
        llmBaseUrl = data.llm_base_url ?? "";
        llmDailyBudget = data.llm_daily_budget_usd ?? "";
        if (llmProvider === "openrouter" && !llmBaseUrl.trim()) {
          llmBaseUrl = OPENROUTER_BASE;
        }
        modelSearch = llmModel;
        cheapModelSearch = llmModelCheap;
        if (llmProvider === "openrouter") loadModels();
        const toggles = {};
        for (const key of DETECTOR_KEYS) {
          const saved = data[`${key}_enabled`];
          toggles[key] = saved !== undefined ? saved === "true" : true;
        }
        detectorToggles = toggles;
        defaultSeverity = data.default_severity ?? "warning";
        maxWarnings = data.max_warnings ?? "20";
        maxBlocking = data.max_blocking ?? "0";
        reviewStrictness = data.review_strictness ?? "balanced";
        forbiddenPaths = data.forbidden_paths ?? "";
        autoLabels = data.auto_labels_enabled !== "false";
        requestReviewers = data.request_reviewers !== "false";
        createCheckRun = data.create_check_run !== "false";
        excludePatterns = data.exclude_patterns ?? "";
        customInstructions = data.custom_instructions ?? "";
        updatePrDescription = data.update_pr_description === "true";
        allowAutoFix = data.allow_auto_fix === "true";
        offlineMode = ["true", "1", "yes", "on"].includes(
          String(data.offline_mode ?? "").toLowerCase()
        );
        publicUrl = data.public_url ?? "";
        auditRetentionDays = data.audit_retention_days ?? "90";
        queueWorkers = data.queue_workers ?? "";
        maxConcurrentReviews = data.max_concurrent_reviews ?? "";
        hsts = ["true", "1", "yes", "on"].includes(String(data.hsts ?? "").toLowerCase());
        metricsToken = data.metrics_token ?? "";
        reviewTimeoutSecs = data.review_timeout_secs ?? "300";
        maxInlineComments = data.max_inline_comments ?? "8";
        maxReviewerFiles = data.max_reviewer_files ?? "8";
        maxCommentBytes = data.max_comment_bytes ?? "64000";
        maxLlmDiffChars = data.max_llm_diff_chars ?? "8000";
        autoImproveMaxFiles = data.auto_improve_max_files ?? "40";
        autoImproveMaxDiff = data.auto_improve_max_diff ?? "24000";
        allowLocalLlm = ["true", "1", "yes", "on"].includes(
          String(data.allow_local_llm ?? "").toLowerCase()
        );
        insecureCookies = ["true", "1", "yes", "on"].includes(
          String(data.insecure_cookies ?? "").toLowerCase()
        );
        secureCookies = ["true", "1", "yes", "on"].includes(
          String(data.secure_cookies ?? "").toLowerCase()
        );
        oidcIssuer = data.oidc_issuer ?? "";
        oidcClientId = data.oidc_client_id ?? "";
        oidcClientSecret = data.oidc_client_secret ?? "";
        oidcRedirectUri = data.oidc_redirect_uri ?? "";
        oidcScopes = data.oidc_scopes ?? "openid email profile";
        oidcAllowOpenJoin = ["true", "1", "yes", "on"].includes(
          String(data.oidc_allow_open_join ?? "").toLowerCase()
        );
        oidcAllowUnverifiedEmail = ["true", "1", "yes", "on"].includes(
          String(data.oidc_allow_unverified_email ?? "").toLowerCase()
        );
        oidcAllowPublicClient = ["true", "1", "yes", "on"].includes(
          String(data.oidc_allow_public_client ?? "").toLowerCase()
        );
        jiraBaseUrl = data.jira_base_url ?? "";
        jiraEmail = data.jira_email ?? "";
        jiraApiToken = data.jira_api_token ?? "";
        linearApiKey = data.linear_api_key ?? "";
        try {
          const lr = await api.get("/api/learning/rules");
          learnedRules = lr.rules || [];
        } catch { learnedRules = []; }
      } catch (err) {
        error = err.message || "Failed to load settings";
      } finally {
        loading = false;
        applyDeepLink();
      }
    })();
  });

  async function saveLLM() {
    llmSaving = true; llmMsg = "";
    try {
      if (modelSearch.trim()) llmModel = modelSearch.trim();
      if (cheapModelSearch.trim()) llmModelCheap = cheapModelSearch.trim();
      if (llmProvider === "openrouter") llmBaseUrl = OPENROUTER_BASE;
      const updates = [
        api.put("/api/settings/llm_provider", { value: llmProvider }),
        api.put("/api/settings/llm_model", { value: llmModel }),
        api.put("/api/settings/llm_model_cheap", { value: llmModelCheap }),
        api.put("/api/settings/llm_base_url", { value: llmBaseUrl }),
        api.put("/api/settings/llm_daily_budget_usd", { value: llmDailyBudget }),
      ];
      if (llmKey && !llmKey.includes("•") && !llmKey.includes("*")) {
        updates.push(api.put("/api/settings/openrouter_api_key", { value: llmKey }));
      }
      const results = await Promise.allSettled(updates);
      const failed = results.filter((r) => r.status === "rejected");
      llmMsg = failed.length === 0 ? "Saved" : `Save failed (${failed.length} errors)`;
    } catch (err) { llmMsg = err.message || "Save failed"; }
    finally { llmSaving = false; }
  }

  async function saveDetectors() {
    detectorSaving = true; detectorMsg = "";
    try {
      const updates = Object.entries(detectorToggles).map(([key, enabled]) =>
        api.put(`/api/settings/${key}_enabled`, { value: enabled ? "true" : "false" })
      );
      const results = await Promise.allSettled(updates);
      const failed = results.filter((r) => r.status === "rejected");
      detectorMsg = failed.length === 0 ? "Saved" : `Save failed (${failed.length} errors)`;
    } catch (err) { detectorMsg = err.message || "Save failed"; }
    finally { detectorSaving = false; }
  }

  async function saveSeverity() {
    severitySaving = true; severityMsg = "";
    try {
      await api.put("/api/settings/default_severity", { value: defaultSeverity });
      severityMsg = "Saved";
    } catch (err) { severityMsg = err.message || "Save failed"; }
    finally { severitySaving = false; }
  }

  async function savePolicy() {
    policySaving = true; policyMsg = "";
    try {
      const updates = [
        api.put("/api/settings/review_strictness", { value: reviewStrictness }),
        api.put("/api/settings/max_warnings", { value: maxWarnings }),
        api.put("/api/settings/max_blocking", { value: maxBlocking }),
        api.put("/api/settings/forbidden_paths", { value: forbiddenPaths }),
        api.put("/api/settings/auto_labels_enabled", { value: autoLabels ? "true" : "false" }),
        api.put("/api/settings/request_reviewers", { value: requestReviewers ? "true" : "false" }),
        api.put("/api/settings/create_check_run", { value: createCheckRun ? "true" : "false" }),
        api.put("/api/settings/exclude_patterns", { value: excludePatterns }),
        api.put("/api/settings/custom_instructions", { value: customInstructions }),
        api.put("/api/settings/update_pr_description", { value: updatePrDescription ? "true" : "false" }),
        api.put("/api/settings/allow_auto_fix", { value: allowAutoFix ? "true" : "false" }),
        api.put("/api/settings/offline_mode", { value: offlineMode ? "true" : "false" }),
      ];
      const results = await Promise.allSettled(updates);
      const failed = results.filter((r) => r.status === "rejected");
      policyMsg = failed.length === 0 ? "Saved" : `Save failed (${failed.length} errors)`;
    } catch (err) { policyMsg = err.message || "Save failed"; }
    finally { policySaving = false; }
  }

  async function deleteRule(id) {
    rulesMsg = "";
    try {
      await api.delete(`/api/learning/rules/${id}`);
      learnedRules = learnedRules.filter((r) => r.id !== id);
      rulesMsg = "Rule deleted";
    } catch (err) {
      rulesMsg = err.message || "Delete failed";
    }
  }

  async function openInstallUrl() {
    githubMsg = "";
    try {
      const data = await api.get("/api/github/install-url");
      if (data.url) window.open(data.url, "_blank");
    } catch (err) {
      githubMsg = err.message || "Failed to open install URL";
    }
  }

  async function clearLocalGithub() {
    clearingGithub = true;
    githubMsg = "";
    try {
      const res = await api.delete("/api/settings/github");
      github = { configured: false };
      confirmClearGithub = false;
      githubMsg =
        res?.message ||
        "Local GitHub App config cleared. Repos are marked inactive. Remove the App on GitHub separately if needed.";
    } catch (err) {
      githubMsg = err.message || "Clear failed";
    } finally {
      clearingGithub = false;
    }
  }

  function boolVal(on) {
    return on ? "true" : "false";
  }

  async function putMany(pairs) {
    const updates = pairs.map(([key, value]) => api.put(`/api/settings/${key}`, { value }));
    const results = await Promise.allSettled(updates);
    const failed = results.filter((r) => r.status === "rejected");
    const restart = results.some(
      (r) => r.status === "fulfilled" && r.value?.restart_required
    );
    return { failed: failed.length, restart };
  }

  async function saveRuntime() {
    runtimeSaving = true;
    runtimeMsg = "";
    try {
      const { failed, restart } = await putMany([
        ["public_url", publicUrl],
        ["audit_retention_days", auditRetentionDays],
        ["queue_workers", queueWorkers],
        ["max_concurrent_reviews", maxConcurrentReviews],
        ["hsts", boolVal(hsts)],
        ["review_timeout_secs", reviewTimeoutSecs],
        ["max_inline_comments", maxInlineComments],
        ["max_reviewer_files", maxReviewerFiles],
        ["max_comment_bytes", maxCommentBytes],
        ["max_llm_diff_chars", maxLlmDiffChars],
        ["auto_improve_max_files", autoImproveMaxFiles],
        ["auto_improve_max_diff", autoImproveMaxDiff],
        ["allow_local_llm", boolVal(allowLocalLlm)],
        ["insecure_cookies", boolVal(insecureCookies)],
        ["secure_cookies", boolVal(secureCookies)],
      ]);
      if (metricsToken && !metricsToken.includes("•") && !metricsToken.includes("*")) {
        await api.put("/api/settings/metrics_token", { value: metricsToken });
      }
      if (failed > 0) runtimeMsg = `Save failed (${failed} errors)`;
      else if (restart) runtimeMsg = "Saved — restart the process to apply worker/concurrency changes";
      else runtimeMsg = "Saved";
    } catch (err) {
      runtimeMsg = err.message || "Save failed";
    } finally {
      runtimeSaving = false;
    }
  }

  async function saveAuth() {
    authSaving = true;
    authMsg = "";
    try {
      const pairs = [
        ["oidc_issuer", oidcIssuer],
        ["oidc_client_id", oidcClientId],
        ["oidc_redirect_uri", oidcRedirectUri],
        ["oidc_scopes", oidcScopes],
        ["oidc_allow_open_join", boolVal(oidcAllowOpenJoin)],
        ["oidc_allow_unverified_email", boolVal(oidcAllowUnverifiedEmail)],
        ["oidc_allow_public_client", boolVal(oidcAllowPublicClient)],
      ];
      if (oidcClientSecret && !oidcClientSecret.includes("•") && !oidcClientSecret.includes("*")) {
        pairs.push(["oidc_client_secret", oidcClientSecret]);
      }
      const { failed } = await putMany(pairs);
      authMsg = failed === 0 ? "Saved" : `Save failed (${failed} errors)`;
    } catch (err) {
      authMsg = err.message || "Save failed";
    } finally {
      authSaving = false;
    }
  }

  async function saveIntegrations() {
    integrationsSaving = true;
    integrationsMsg = "";
    try {
      const pairs = [
        ["jira_base_url", jiraBaseUrl],
        ["jira_email", jiraEmail],
      ];
      if (jiraApiToken && !jiraApiToken.includes("•") && !jiraApiToken.includes("*")) {
        pairs.push(["jira_api_token", jiraApiToken]);
      }
      if (linearApiKey && !linearApiKey.includes("•") && !linearApiKey.includes("*")) {
        pairs.push(["linear_api_key", linearApiKey]);
      }
      const { failed } = await putMany(pairs);
      integrationsMsg = failed === 0 ? "Saved" : `Save failed (${failed} errors)`;
    } catch (err) {
      integrationsMsg = err.message || "Save failed";
    } finally {
      integrationsSaving = false;
    }
  }
</script>

<AppShell title="Settings">
  <LoadingSpinner loading={loading} />
  {#if error}
    <ErrorState message={error} />
  {:else if loading}
  {:else}
    <div class="page-toolbar compact settings-hero">
      <div>
        <h1 class="page-title">Settings</h1>
        <p class="page-description">LLM, detectors, policy, runtime, SSO, integrations, GitHub App, and account.</p>
      </div>
    </div>

    {#if !canEditSettings}
      <div class="settings-readonly-banner" role="status">
        Only owners can change LLM, detector, policy, and GitHub settings. Your role: {roleLabel($currentUser?.role, $currentUser?.is_bootstrap)}.
      </div>
    {/if}

    <div class="settings-tabs-wrap">
      <nav class="settings-tabs" aria-label="Settings tabs" role="tablist">
        {#each TABS as s}
          <button
            type="button"
            role="tab"
            class="settings-tab"
            class:active={activeTab === s.id}
            id="tab-{s.id}"
            aria-selected={activeTab === s.id}
            aria-controls="panel-{s.id}"
            tabindex={activeTab === s.id ? 0 : -1}
            onclick={() => selectTab(s.id)}
          >{s.label}</button>
        {/each}
      </nav>
    </div>

    <div
      class="settings-panel"
      role="tabpanel"
      id="panel-{activeTab}"
      aria-labelledby="tab-{activeTab}"
    >
      {#if activeTab === "llm"}
          <section class="card settings-card">
            <header class="settings-section-head">
              <h3 class="section-heading">LLM</h3>
              <p class="section-desc">Optional. Tier-1 detectors run without a model.</p>
            </header>
            <div class="form-group">
              <label for="llm-provider">Provider</label>
              <select id="llm-provider" bind:value={llmProvider} onchange={(e) => handleProviderChange(e.target.value)}>
                <option value="openrouter">OpenRouter</option>
                <option value="ollama">Ollama</option>
                <option value="custom">Custom</option>
                <option value="disabled">Disabled</option>
              </select>
            </div>
            {#if llmProvider !== "disabled"}
              <div class="form-group">
                <label for="llm-key">API Key</label>
                <input id="llm-key" type="password" bind:value={llmKey} placeholder="sk-..." />
              </div>
              <div class="settings-llm-grid">
                <div class="form-group model-search">
                  <label for="llm-model">Model (review_diff)</label>
                  {#if llmProvider === "openrouter"}
                    <div class="search-wrap">
                      <input id="llm-model" type="text" bind:value={modelSearch}
                        oninput={() => (modelDropdown = true)}
                        onfocus={() => (modelDropdown = true)}
                        onkeydown={handleModelKeydown}
                        onblur={handleModelBlur}
                        placeholder="Search models…" autocomplete="off" />
                      {#if modelDropdown && modelFiltered.length > 0}
                        <div class="search-dropdown">
                          {#each modelFiltered as m}
                            <button class="search-item" class:active={m.id === llmModel}
                              onmousedown={(e) => e.preventDefault()}
                              onclick={() => selectModel(m)}>
                              <span class="search-id">{m.id}</span>
                            </button>
                          {/each}
                        </div>
                      {/if}
                    </div>
                  {:else}
                    <input id="llm-model" type="text" bind:value={llmModel} />
                  {/if}
                </div>
                <div class="form-group model-search">
                  <label for="llm-model-cheap">Cheap model (summarize / describe / ask)</label>
                  {#if llmProvider === "openrouter"}
                    <div class="search-wrap">
                      <input id="llm-model-cheap" type="text" bind:value={cheapModelSearch}
                        oninput={() => (cheapModelDropdown = true)}
                        onfocus={() => (cheapModelDropdown = true)}
                        onkeydown={handleCheapModelKeydown}
                        onblur={handleCheapModelBlur}
                        placeholder="Search models…" autocomplete="off" />
                      {#if cheapModelDropdown && cheapModelFiltered.length > 0}
                        <div class="search-dropdown">
                          {#each cheapModelFiltered as m}
                            <button class="search-item" class:active={m.id === llmModelCheap}
                              onmousedown={(e) => e.preventDefault()}
                              onclick={() => selectCheapModel(m)}>
                              <span class="search-id">{m.id}</span>
                            </button>
                          {/each}
                        </div>
                      {/if}
                    </div>
                  {:else}
                    <input id="llm-model-cheap" type="text" bind:value={llmModelCheap}
                      placeholder="optional cheaper model id" />
                  {/if}
                </div>
              </div>
              <div class="form-group">
                <label for="llm-url">Base URL</label>
                {#if llmProvider === "openrouter"}
                  <input id="llm-url" type="text" value={OPENROUTER_BASE} readonly class="input-readonly" />
                  <p class="field-hint">Fixed for OpenRouter. Saved automatically.</p>
                {:else}
                  <input id="llm-url" type="text" bind:value={llmBaseUrl} placeholder="https://…" />
                {/if}
              </div>
              <div class="form-group">
                <label for="llm-budget">Daily LLM budget USD (0 = unlimited)</label>
                <input id="llm-budget" type="text" bind:value={llmDailyBudget} placeholder="e.g. 5" />
              </div>
            {/if}
            <div class="save-row">
              <button onclick={saveLLM} disabled={llmSaving || !canEditSettings}>{llmSaving ? "Saving…" : "Save"}</button>
              {#if llmMsg}<span class="save-msg" class:error={llmMsg !== "Saved"}>{llmMsg}</span>{/if}
            </div>
          </section>
          {:else if activeTab === "detectors"}
          <section class="card settings-card">
            <header class="settings-section-head">
              <h3 class="section-heading">Detectors</h3>
              <p class="section-desc">Tier-1 checks that run on every PR.</p>
            </header>
            <div class="detector-list">
              {#each Object.entries(detectorToggles) as [key, val]}
                <div class="detector-row">
                  <span>{formatLabel(key)}</span>
                  <label class="toggle">
                    <div class="toggle-track" class:on={val ?? false} role="checkbox" aria-checked={val ?? false}
                      tabindex="0"
                      onclick={() => (detectorToggles[key] = !(detectorToggles[key] ?? false))}
                      onkeydown={(e) => { if (e.key === 'Enter') detectorToggles[key] = !(detectorToggles[key] ?? false); }}>
                      <div class="toggle-knob"></div>
                    </div>
                  </label>
                </div>
              {/each}
            </div>
            <div class="form-group" style="margin-top:16px">
              <label for="default-severity">Minimum severity to surface</label>
              <select id="default-severity" bind:value={defaultSeverity}>
                <option value="blocking">Blocking</option>
                <option value="warning">Warning</option>
                <option value="info">Info</option>
              </select>
            </div>
            <div class="save-row">
              <button onclick={async () => { await saveDetectors(); await saveSeverity(); }} disabled={detectorSaving || severitySaving || !canEditSettings}>
                {detectorSaving || severitySaving ? "Saving…" : "Save"}
              </button>
              {#if detectorMsg || severityMsg}
                <span class="save-msg" class:error={(detectorMsg && detectorMsg !== "Saved") || (severityMsg && severityMsg !== "Saved")}>
                  {detectorMsg === "Saved" && (!severityMsg || severityMsg === "Saved") ? "Saved" : (detectorMsg || severityMsg)}
                </span>
              {/if}
            </div>
          </section>
          {:else if activeTab === "policy"}
          <section class="card settings-card">
            <header class="settings-section-head">
              <h3 class="section-heading">Policy</h3>
              <p class="section-desc">Strictness, caps, paths, labels, and offline mode.</p>
            </header>
            <div class="form-group">
              <label for="review-strictness">Review strictness</label>
              <select id="review-strictness" bind:value={reviewStrictness}>
                <option value="lenient">Lenient — fewer nits, warnings+</option>
                <option value="balanced">Balanced — default</option>
                <option value="strict">Strict — more thorough</option>
                <option value="nitpick">Nitpick — include style/info</option>
              </select>
              <p class="section-desc" style="margin-top:6px">Also set via <code>[behavior] review_strictness</code> in <code>.codasaurus.toml</code>.</p>
            </div>
            <div class="form-group">
              <label for="max-warnings">Max warnings (soft cap)</label>
              <input id="max-warnings" type="number" min="0" bind:value={maxWarnings} />
            </div>
            <div class="form-group">
              <label for="max-blocking">Max blocking findings</label>
              <input id="max-blocking" type="number" min="0" bind:value={maxBlocking} />
            </div>
            <div class="form-group">
              <label for="forbidden-paths">Forbidden path prefixes</label>
              <input id="forbidden-paths" type="text" bind:value={forbiddenPaths} placeholder="vendor/,secrets/" />
            </div>
            <div class="detector-row" style="border:none;padding:8px 0">
              <span>Auto-apply PR labels</span>
              <label class="toggle">
                <div class="toggle-track" class:on={autoLabels} role="checkbox" aria-checked={autoLabels}
                  tabindex="0"
                  onclick={() => (autoLabels = !autoLabels)}
                  onkeydown={(e) => { if (e.key === 'Enter') autoLabels = !autoLabels; }}>
                  <div class="toggle-knob"></div>
                </div>
              </label>
            </div>
            <div class="detector-row" style="border:none;padding:8px 0">
              <span>Request CODEOWNERS reviewers</span>
              <label class="toggle">
                <div class="toggle-track" class:on={requestReviewers} role="checkbox" aria-checked={requestReviewers}
                  tabindex="0"
                  onclick={() => (requestReviewers = !requestReviewers)}
                  onkeydown={(e) => { if (e.key === 'Enter') requestReviewers = !requestReviewers; }}>
                  <div class="toggle-knob"></div>
                </div>
              </label>
            </div>
            <div class="detector-row" style="border:none;padding:8px 0">
              <span>Create Check Runs</span>
              <label class="toggle">
                <div class="toggle-track" class:on={createCheckRun} role="checkbox" aria-checked={createCheckRun}
                  tabindex="0"
                  onclick={() => (createCheckRun = !createCheckRun)}
                  onkeydown={(e) => { if (e.key === 'Enter') createCheckRun = !createCheckRun; }}>
                  <div class="toggle-knob"></div>
                </div>
              </label>
            </div>
            <div class="detector-row" style="border:none;padding:8px 0">
              <span>Update PR description on describe</span>
              <label class="toggle">
                <div class="toggle-track" class:on={updatePrDescription} role="checkbox" aria-checked={updatePrDescription}
                  tabindex="0"
                  onclick={() => (updatePrDescription = !updatePrDescription)}
                  onkeydown={(e) => { if (e.key === 'Enter') updatePrDescription = !updatePrDescription; }}>
                  <div class="toggle-knob"></div>
                </div>
              </label>
            </div>
            <div class="detector-row" style="border:none;padding:8px 0">
              <span>Allow @codasaurus fix (writes to PR branch)</span>
              <label class="toggle">
                <div class="toggle-track" class:on={allowAutoFix} role="checkbox" aria-checked={allowAutoFix}
                  tabindex="0"
                  onclick={() => (allowAutoFix = !allowAutoFix)}
                  onkeydown={(e) => { if (e.key === 'Enter') allowAutoFix = !allowAutoFix; }}>
                  <div class="toggle-knob"></div>
                </div>
              </label>
            </div>
            <div class="detector-row" style="border:none;padding:8px 0">
              <span>Offline / air-gap mode</span>
              <label class="toggle">
                <div class="toggle-track" class:on={offlineMode} role="checkbox" aria-checked={offlineMode}
                  tabindex="0"
                  onclick={() => (offlineMode = !offlineMode)}
                  onkeydown={(e) => { if (e.key === 'Enter') offlineMode = !offlineMode; }}>
                  <div class="toggle-knob"></div>
                </div>
              </label>
            </div>
            <div class="form-group">
              <label for="exclude-patterns">Exclude path patterns</label>
              <input id="exclude-patterns" type="text" bind:value={excludePatterns} placeholder="vendor/,*.lock,dist/" />
            </div>
            <div class="form-group">
              <label for="custom-instructions">Org custom instructions (LLM)</label>
              <textarea id="custom-instructions" rows="4" bind:value={customInstructions} placeholder="Prefer small PRs; never suggest rewriting auth…"></textarea>
            </div>
            <div class="save-row">
              <button onclick={savePolicy} disabled={policySaving || !canEditSettings}>{policySaving ? "Saving…" : "Save"}</button>
              {#if policyMsg}<span class="save-msg" class:error={policyMsg !== "Saved"}>{policyMsg}</span>{/if}
            </div>
          </section>
          {:else if activeTab === "runtime"}
          <section class="card settings-card">
            <header class="settings-section-head">
              <h3 class="section-heading">Runtime &amp; security</h3>
              <p class="section-desc">
                Same knobs as env vars. Host env wins on restart if set; saving here applies immediately for most settings.
                Queue workers and max concurrent reviews need a process restart.
              </p>
            </header>
            <div class="form-group">
              <label for="public-url">Public URL</label>
              <input id="public-url" type="url" bind:value={publicUrl} placeholder="https://reviews.example.com" disabled={!canEditSettings} />
              <p class="field-hint">Canonical origin for OIDC redirects, invites, and HSTS when https.</p>
            </div>
            <div class="form-row-2">
              <div class="form-group">
                <label for="audit-retention">Audit retention (days)</label>
                <input id="audit-retention" type="number" min="7" max="730" bind:value={auditRetentionDays} disabled={!canEditSettings} />
              </div>
              <div class="form-group">
                <label for="review-timeout">Review timeout (secs)</label>
                <input id="review-timeout" type="number" min="30" max="3600" bind:value={reviewTimeoutSecs} disabled={!canEditSettings} />
              </div>
            </div>
            <div class="form-row-2">
              <div class="form-group">
                <label for="queue-workers">Queue workers (1–8)</label>
                <input id="queue-workers" type="number" min="1" max="8" bind:value={queueWorkers} placeholder="auto" disabled={!canEditSettings} />
                <p class="field-hint">Requires restart</p>
              </div>
              <div class="form-group">
                <label for="max-concurrent">Max concurrent reviews</label>
                <input id="max-concurrent" type="number" min="1" max="64" bind:value={maxConcurrentReviews} placeholder="4" disabled={!canEditSettings} />
                <p class="field-hint">Requires restart</p>
              </div>
            </div>
            <div class="form-row-2">
              <div class="form-group">
                <label for="max-inline">Max inline comments</label>
                <input id="max-inline" type="number" bind:value={maxInlineComments} disabled={!canEditSettings} />
              </div>
              <div class="form-group">
                <label for="max-reviewer-files">Max reviewer files</label>
                <input id="max-reviewer-files" type="number" bind:value={maxReviewerFiles} disabled={!canEditSettings} />
              </div>
            </div>
            <div class="form-row-2">
              <div class="form-group">
                <label for="max-comment-bytes">Max comment bytes</label>
                <input id="max-comment-bytes" type="number" bind:value={maxCommentBytes} disabled={!canEditSettings} />
              </div>
              <div class="form-group">
                <label for="max-llm-diff">Max LLM diff chars</label>
                <input id="max-llm-diff" type="number" bind:value={maxLlmDiffChars} disabled={!canEditSettings} />
              </div>
            </div>
            <div class="form-row-2">
              <div class="form-group">
                <label for="auto-improve-files">Auto-improve max files</label>
                <input id="auto-improve-files" type="number" bind:value={autoImproveMaxFiles} disabled={!canEditSettings} />
              </div>
              <div class="form-group">
                <label for="auto-improve-diff">Auto-improve max diff chars</label>
                <input id="auto-improve-diff" type="number" bind:value={autoImproveMaxDiff} disabled={!canEditSettings} />
              </div>
            </div>
            <div class="form-group">
              <label for="metrics-token">Metrics bearer token</label>
              <input id="metrics-token" type="password" bind:value={metricsToken} placeholder="Leave blank to disable /metrics" disabled={!canEditSettings} autocomplete="off" />
            </div>
            <div class="detector-row" style="border:none;padding:8px 0">
              <span>Force HSTS</span>
              <label class="toggle">
                <div class="toggle-track" class:on={hsts} role="checkbox" aria-checked={hsts}
                  tabindex="0"
                  onclick={() => canEditSettings && (hsts = !hsts)}
                  onkeydown={(e) => { if (canEditSettings && e.key === 'Enter') hsts = !hsts; }}>
                  <div class="toggle-knob"></div>
                </div>
              </label>
            </div>
            <div class="detector-row" style="border:none;padding:8px 0">
              <span>Allow local LLM endpoints</span>
              <label class="toggle">
                <div class="toggle-track" class:on={allowLocalLlm} role="checkbox" aria-checked={allowLocalLlm}
                  tabindex="0"
                  onclick={() => canEditSettings && (allowLocalLlm = !allowLocalLlm)}
                  onkeydown={(e) => { if (canEditSettings && e.key === 'Enter') allowLocalLlm = !allowLocalLlm; }}>
                  <div class="toggle-knob"></div>
                </div>
              </label>
            </div>
            <div class="detector-row" style="border:none;padding:8px 0">
              <span>Force Secure cookies</span>
              <label class="toggle">
                <div class="toggle-track" class:on={secureCookies} role="checkbox" aria-checked={secureCookies}
                  tabindex="0"
                  onclick={() => canEditSettings && (secureCookies = !secureCookies)}
                  onkeydown={(e) => { if (canEditSettings && e.key === 'Enter') secureCookies = !secureCookies; }}>
                  <div class="toggle-knob"></div>
                </div>
              </label>
            </div>
            <div class="detector-row" style="border:none;padding:8px 0">
              <span>Insecure cookies (HTTP only)</span>
              <label class="toggle">
                <div class="toggle-track" class:on={insecureCookies} role="checkbox" aria-checked={insecureCookies}
                  tabindex="0"
                  onclick={() => canEditSettings && (insecureCookies = !insecureCookies)}
                  onkeydown={(e) => { if (canEditSettings && e.key === 'Enter') insecureCookies = !insecureCookies; }}>
                  <div class="toggle-knob"></div>
                </div>
              </label>
            </div>
            <div class="save-row">
              <button onclick={saveRuntime} disabled={runtimeSaving || !canEditSettings}>{runtimeSaving ? "Saving…" : "Save"}</button>
              {#if runtimeMsg}<span class="save-msg" class:error={!runtimeMsg.startsWith("Saved")}>{runtimeMsg}</span>{/if}
            </div>
          </section>
          {:else if activeTab === "auth"}
          <section class="card settings-card">
            <header class="settings-section-head">
              <h3 class="section-heading">OIDC / SSO</h3>
              <p class="section-desc">Configure an identity provider. Equivalent to OIDC_* environment variables.</p>
            </header>
            <div class="form-group">
              <label for="oidc-issuer">Issuer URL</label>
              <input id="oidc-issuer" type="url" bind:value={oidcIssuer} placeholder="https://accounts.example.com" disabled={!canEditSettings} />
            </div>
            <div class="form-row-2">
              <div class="form-group">
                <label for="oidc-client-id">Client ID</label>
                <input id="oidc-client-id" type="text" bind:value={oidcClientId} disabled={!canEditSettings} />
              </div>
              <div class="form-group">
                <label for="oidc-client-secret">Client secret</label>
                <input id="oidc-client-secret" type="password" bind:value={oidcClientSecret} disabled={!canEditSettings} autocomplete="off" />
              </div>
            </div>
            <div class="form-group">
              <label for="oidc-redirect">Redirect URI (optional)</label>
              <input id="oidc-redirect" type="url" bind:value={oidcRedirectUri} placeholder="Defaults to PUBLIC_URL/api/auth/oidc/callback" disabled={!canEditSettings} />
            </div>
            <div class="form-group">
              <label for="oidc-scopes">Scopes</label>
              <input id="oidc-scopes" type="text" bind:value={oidcScopes} disabled={!canEditSettings} />
            </div>
            <div class="detector-row" style="border:none;padding:8px 0">
              <span>Allow open join (no invite)</span>
              <label class="toggle">
                <div class="toggle-track" class:on={oidcAllowOpenJoin} role="checkbox" aria-checked={oidcAllowOpenJoin}
                  tabindex="0"
                  onclick={() => canEditSettings && (oidcAllowOpenJoin = !oidcAllowOpenJoin)}
                  onkeydown={(e) => { if (canEditSettings && e.key === 'Enter') oidcAllowOpenJoin = !oidcAllowOpenJoin; }}>
                  <div class="toggle-knob"></div>
                </div>
              </label>
            </div>
            <div class="detector-row" style="border:none;padding:8px 0">
              <span>Allow unverified email claims</span>
              <label class="toggle">
                <div class="toggle-track" class:on={oidcAllowUnverifiedEmail} role="checkbox" aria-checked={oidcAllowUnverifiedEmail}
                  tabindex="0"
                  onclick={() => canEditSettings && (oidcAllowUnverifiedEmail = !oidcAllowUnverifiedEmail)}
                  onkeydown={(e) => { if (canEditSettings && e.key === 'Enter') oidcAllowUnverifiedEmail = !oidcAllowUnverifiedEmail; }}>
                  <div class="toggle-knob"></div>
                </div>
              </label>
            </div>
            <div class="detector-row" style="border:none;padding:8px 0">
              <span>Allow public client (empty secret)</span>
              <label class="toggle">
                <div class="toggle-track" class:on={oidcAllowPublicClient} role="checkbox" aria-checked={oidcAllowPublicClient}
                  tabindex="0"
                  onclick={() => canEditSettings && (oidcAllowPublicClient = !oidcAllowPublicClient)}
                  onkeydown={(e) => { if (canEditSettings && e.key === 'Enter') oidcAllowPublicClient = !oidcAllowPublicClient; }}>
                  <div class="toggle-knob"></div>
                </div>
              </label>
            </div>
            <div class="save-row">
              <button onclick={saveAuth} disabled={authSaving || !canEditSettings}>{authSaving ? "Saving…" : "Save"}</button>
              {#if authMsg}<span class="save-msg" class:error={authMsg !== "Saved"}>{authMsg}</span>{/if}
            </div>
          </section>
          {:else if activeTab === "integrations"}
          <section class="card settings-card">
            <header class="settings-section-head">
              <h3 class="section-heading">Ticket integrations</h3>
              <p class="section-desc">Enrich PR context from Jira keys and Linear issue IDs in the description.</p>
            </header>
            <div class="form-group">
              <label for="jira-base">Jira base URL</label>
              <input id="jira-base" type="url" bind:value={jiraBaseUrl} placeholder="https://your-org.atlassian.net" disabled={!canEditSettings} />
            </div>
            <div class="form-row-2">
              <div class="form-group">
                <label for="jira-email">Jira email</label>
                <input id="jira-email" type="email" bind:value={jiraEmail} disabled={!canEditSettings} />
              </div>
              <div class="form-group">
                <label for="jira-token">Jira API token</label>
                <input id="jira-token" type="password" bind:value={jiraApiToken} disabled={!canEditSettings} autocomplete="off" />
              </div>
            </div>
            <div class="form-group">
              <label for="linear-key">Linear API key</label>
              <input id="linear-key" type="password" bind:value={linearApiKey} disabled={!canEditSettings} autocomplete="off" />
            </div>
            <div class="save-row">
              <button onclick={saveIntegrations} disabled={integrationsSaving || !canEditSettings}>{integrationsSaving ? "Saving…" : "Save"}</button>
              {#if integrationsMsg}<span class="save-msg" class:error={integrationsMsg !== "Saved"}>{integrationsMsg}</span>{/if}
            </div>
          </section>
          {:else if activeTab === "github"}
          <section class="card settings-card">
            <header class="settings-section-head">
              <h3 class="section-heading">GitHub App</h3>
              <p class="section-desc">Install URL and local credentials. Rotate keys on GitHub, then clear and re-setup here.</p>
            </header>
            {#if github?.configured}
              <div class="settings-meta-grid">
                <div>
                  <span class="meta-label">App name</span>
                  <p class="meta-value">{github.app_name ?? "-"}</p>
                </div>
                <div>
                  <span class="meta-label">App ID</span>
                  <p class="meta-value">{github.app_id ?? "-"}</p>
                </div>
              </div>
              <div class="save-row" style="margin-top:8px">
                <button onclick={openInstallUrl}>Open install URL</button>
                <a
                  href="https://github.com/settings/apps"
                  target="_blank"
                  rel="noopener noreferrer"
                  class="btn-link"
                >Manage on GitHub ↗</a>
              </div>
              <p class="field-hint">
                To rotate the private key or webhook secret, generate new credentials in GitHub, clear local config below, then re-run setup (or set env vars and redeploy).
              </p>
              {#if githubMsg}
                <p class="save-msg" class:error={/fail|error/i.test(githubMsg)}>{githubMsg}</p>
              {/if}
              <div class="danger-zone" style="margin-top:20px">
                <h3>Danger zone</h3>
                <p>
                  Clears App ID, private key, and webhook secret from Codasaurus and marks synced repos inactive.
                  This does <strong>not</strong> uninstall the App from GitHub.
                </p>
                {#if !confirmClearGithub}
                  <button class="danger" onclick={() => (confirmClearGithub = true)} disabled={!canEditSettings}>Clear local GitHub config</button>
                {:else}
                  <p style="font-size:13px;margin-bottom:8px">Clear local credentials? The GitHub App stays installed until you remove it on GitHub.</p>
                  <div style="display:flex;gap:8px">
                    <button class="danger" onclick={clearLocalGithub} disabled={clearingGithub}>
                      {clearingGithub ? "Clearing…" : "Confirm clear"}
                    </button>
                    <button onclick={() => (confirmClearGithub = false)}>Cancel</button>
                  </div>
                {/if}
              </div>
            {:else}
              <p class="empty-note">No GitHub App configured in Codasaurus yet.</p>
              {#if githubMsg}
                <p class="save-msg" class:error={/fail|error/i.test(githubMsg)}>{githubMsg}</p>
              {/if}
              <div class="save-row">
                <button class="primary" onclick={openInstallUrl}>Install GitHub App</button>
              </div>
            {/if}
          </section>
          {:else if activeTab === "account"}
          <section class="card settings-card">
            <header class="settings-section-head">
              <h3 class="section-heading">Account</h3>
              <p class="section-desc">
                Signed in as {roleLabel($currentUser?.role, $currentUser?.is_bootstrap)} · {$currentUser?.email}.
                Manage members on the <a href="#/app/team">Team</a> page.
              </p>
            </header>
            {#if $currentUser?.auth_provider !== "oidc"}
              <div class="form-group">
                <label for="pw-current">Current password</label>
                <input id="pw-current" type="password" bind:value={pwCurrent} autocomplete="current-password" />
              </div>
              <div class="form-group">
                <label for="pw-new">New password</label>
                <input id="pw-new" type="password" bind:value={pwNew} placeholder="At least 10 characters" autocomplete="new-password" />
              </div>
              <div class="save-row">
                <button onclick={changePassword} disabled={pwSaving || !pwCurrent || pwNew.length < 10}>
                  {pwSaving ? "Updating…" : "Update password"}
                </button>
                {#if pwMsg}<span class="save-msg" class:error={pwMsg !== "Password updated"}>{pwMsg}</span>{/if}
              </div>
            {:else}
              <p class="empty-note">SSO account — password is managed by your identity provider.</p>
            {/if}
          </section>
          {:else if activeTab === "learning"}
          <section class="card settings-card">
            <header class="settings-section-head">
              <h3 class="section-heading">Learning</h3>
              <p class="section-desc">Ignore rules taught by dismissing findings.</p>
            </header>
            {#if learnedRules.length === 0}
              <p class="empty-note">No learned ignore rules yet. Dismiss findings to teach Codasaurus.</p>
            {:else}
              <div class="detector-list">
                {#each learnedRules as rule}
                  <div class="detector-row">
                    <span>
                      <strong>{rule.detector}</strong>
                      {#if rule.file_pattern} · <code>{rule.file_pattern}</code>{/if}
                      <br /><span class="muted">{rule.reason || rule.action}</span>
                    </span>
                    <button class="linkish" onclick={() => deleteRule(rule.id)} disabled={!$isMaintainer}>Delete</button>
                  </div>
                {/each}
              </div>
            {/if}
            {#if rulesMsg}<p class="save-msg" class:error={rulesMsg !== "Rule deleted"}>{rulesMsg}</p>{/if}
          </section>
          {/if}
        </div>
      {/if}
</AppShell>

<style>
  .settings-hero { margin-bottom: 12px; }
  .settings-section-head { margin-bottom: 16px; }
  .section-desc {
    margin: 4px 0 0;
    font-size: 13px;
    color: var(--text-muted);
  }
  .settings-meta-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 16px;
    margin-bottom: 8px;
  }
  .meta-label {
    display: block;
    font-size: 12px;
    color: var(--text-muted);
    margin-bottom: 4px;
  }
  .meta-value {
    margin: 0;
    font-size: 14px;
    font-family: var(--font-mono);
  }
  .detector-list {
    max-height: 320px;
    overflow-y: auto;
    margin: 0 -24px;
    padding: 0 24px;
    scrollbar-width: thin;
    scrollbar-color: var(--text-muted) var(--bg-secondary);
  }
  .detector-list::-webkit-scrollbar { width: 6px; }
  .detector-list::-webkit-scrollbar-track { background: var(--bg-secondary); border-radius: 3px; }
  .detector-list::-webkit-scrollbar-thumb { background: var(--text-muted); border-radius: 3px; }
  .detector-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 0;
    border-bottom: 1px solid var(--border-light);
    font-size: 14px;
  }
  .detector-row:last-child { border-bottom: none; }
  .search-wrap { position: relative; }
  .search-dropdown {
    position: absolute;
    top: 100%;
    left: 0;
    right: 0;
    z-index: 20;
    max-height: 240px;
    overflow-y: auto;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 6px;
    margin-top: 4px;
    box-shadow: var(--shadow-md);
  }
  .search-item {
    display: block;
    width: 100%;
    text-align: left;
    padding: 8px 12px;
    border: none;
    border-radius: 0;
    background: none;
    font-size: 13px;
    font-family: var(--font-mono);
    color: var(--text-primary);
    cursor: pointer;
  }
  .search-item:hover, .search-item.active { background: var(--bg-secondary); }
  .empty-note { font-size: 13px; color: var(--text-muted); }
  .form-row-2 {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px 16px;
  }
  @media (max-width: 720px) {
    .form-row-2 { grid-template-columns: 1fr; }
  }
  .muted { font-size: 12px; color: var(--text-muted); }
  .linkish {
    background: none;
    border: none;
    color: var(--accent-soft);
    cursor: pointer;
    font-size: 13px;
    padding: 4px 8px;
  }
  .linkish:hover { text-decoration: underline; }
  textarea {
    width: 100%;
    font-family: inherit;
    font-size: 13px;
    padding: 8px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg-primary);
    color: var(--text-primary);
  }
  @media (max-width: 720px) {
    .settings-meta-grid { grid-template-columns: 1fr; }
  }
</style>
