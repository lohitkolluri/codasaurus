<script>
  import { onMount, tick } from "svelte";
  import { location } from "svelte-spa-router";
  import { api } from "../../stores/api.js";
  import { formatLabel } from "../../lib/utils.js";
  import Sidebar from "../../lib/Sidebar.svelte";
  import Header from "../../lib/Header.svelte";
  import LoadingSpinner from "../../lib/LoadingSpinner.svelte";
  import ErrorState from "../../lib/ErrorState.svelte";

  const SECTIONS = [
    { id: "llm", label: "LLM" },
    { id: "detectors", label: "Detectors" },
    { id: "policy", label: "Policy" },
    { id: "github", label: "GitHub" },
    { id: "learning", label: "Learning" },
  ];

  let settings = $state({});
  let loading = $state(true);
  let error = $state("");
  let activeSection = $state("llm");

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

  /** In-page jump only — do not touch location.hash (svelte-spa-router owns it). */
  function scrollToSection(id) {
    activeSection = id;
    document.getElementById(`settings-${id}`)?.scrollIntoView({ behavior: "smooth", block: "start" });
  }

  async function jumpToDeepLink() {
    await tick();
    if ($location === "/app/settings/github") {
      activeSection = "github";
      document.getElementById("settings-github")?.scrollIntoView({ behavior: "smooth", block: "start" });
    }
  }

  function watchSectionVisibility() {
    const nodes = SECTIONS.map((s) => document.getElementById(`settings-${s.id}`)).filter(Boolean);
    if (nodes.length === 0) return () => {};
    const observer = new IntersectionObserver(
      (entries) => {
        const visible = entries
          .filter((e) => e.isIntersecting)
          .sort((a, b) => b.intersectionRatio - a.intersectionRatio);
        if (visible[0]?.target?.id) {
          activeSection = visible[0].target.id.replace(/^settings-/, "");
        }
      },
      { rootMargin: "-20% 0px -55% 0px", threshold: [0.15, 0.4, 0.7] }
    );
    for (const el of nodes) observer.observe(el);
    return () => observer.disconnect();
  }

  onMount(() => {
    let stopWatch = () => {};
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
        try {
          const lr = await api.get("/api/learning/rules");
          learnedRules = lr.rules || [];
        } catch { learnedRules = []; }
      } catch (err) {
        error = err.message || "Failed to load settings";
      } finally {
        loading = false;
        await jumpToDeepLink();
        stopWatch = watchSectionVisibility();
      }
    })();
    return () => stopWatch();
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
</script>

<div class="app-layout">
  <Sidebar />
  <div class="app-main">
    <Header title="Settings" />
    <div class="app-content">
      <LoadingSpinner loading={loading} />
      {#if error}
        <ErrorState message={error} />
      {:else if loading}
      {:else}
        <div class="page-toolbar compact settings-hero">
          <div>
            <h1 class="page-title">Settings</h1>
            <p class="page-description">LLM, detectors, policy, GitHub App, and learned rules.</p>
          </div>
        </div>

        <nav class="settings-subnav" aria-label="Settings sections">
          {#each SECTIONS as s}
            <button
              type="button"
              class="settings-subnav-item"
              class:active={activeSection === s.id}
              aria-current={activeSection === s.id ? "true" : undefined}
              onclick={() => scrollToSection(s.id)}
            >{s.label}</button>
          {/each}
        </nav>

        <div class="settings-stack">
          <section id="settings-llm" class="card settings-card settings-card-wide">
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
              <button onclick={saveLLM} disabled={llmSaving}>{llmSaving ? "Saving…" : "Save"}</button>
              {#if llmMsg}<span class="save-msg" class:error={llmMsg !== "Saved"}>{llmMsg}</span>{/if}
            </div>
          </section>

          <section id="settings-detectors" class="card settings-card">
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
              <button onclick={async () => { await saveDetectors(); await saveSeverity(); }} disabled={detectorSaving || severitySaving}>
                {detectorSaving || severitySaving ? "Saving…" : "Save"}
              </button>
              {#if detectorMsg || severityMsg}
                <span class="save-msg" class:error={(detectorMsg && detectorMsg !== "Saved") || (severityMsg && severityMsg !== "Saved")}>
                  {detectorMsg === "Saved" && (!severityMsg || severityMsg === "Saved") ? "Saved" : (detectorMsg || severityMsg)}
                </span>
              {/if}
            </div>
          </section>

          <section id="settings-policy" class="card settings-card">
            <header class="settings-section-head">
              <h3 class="section-heading">Policy</h3>
              <p class="section-desc">Caps, paths, labels, and offline mode.</p>
            </header>
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
              <button onclick={savePolicy} disabled={policySaving}>{policySaving ? "Saving…" : "Save"}</button>
              {#if policyMsg}<span class="save-msg" class:error={policyMsg !== "Saved"}>{policyMsg}</span>{/if}
            </div>
          </section>

          <section id="settings-github" class="card settings-card settings-card-wide">
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
                  <button class="danger" onclick={() => (confirmClearGithub = true)}>Clear local GitHub config</button>
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

          <section id="settings-learning" class="card settings-card">
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
                    <button class="linkish" onclick={() => deleteRule(rule.id)}>Delete</button>
                  </div>
                {/each}
              </div>
            {/if}
            {#if rulesMsg}<p class="save-msg" class:error={rulesMsg !== "Rule deleted"}>{rulesMsg}</p>{/if}
          </section>
        </div>
      {/if}
    </div>
  </div>
</div>

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
