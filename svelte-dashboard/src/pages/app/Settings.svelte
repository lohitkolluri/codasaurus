<script>
  import { onMount } from "svelte";
  import { location, push } from "svelte-spa-router";
  import { api } from "../../stores/api.js";
  import { currentUser, isOwner, isMaintainer, roleLabel } from "../../stores/auth.js";
  import AppShell from "../../lib/AppShell.svelte";
  import LoadingSpinner from "../../lib/LoadingSpinner.svelte";
  import ErrorState from "../../lib/ErrorState.svelte";
  import Pagination from "../../lib/Pagination.svelte";

  const RULES_PAGE_SIZE = 20;

  // Fewer primary categories; rare knobs live under Advanced (progressive disclosure).
  const TABS = [
    { id: "llm", label: "LLM", hint: "Provider and models" },
    { id: "review", label: "Review", hint: "Detectors and policy" },
    { id: "connections", label: "Connections", hint: "GitHub, SSO, tickets" },
    { id: "system", label: "System", hint: "URL, retention, advanced" },
    { id: "account", label: "Account", hint: "Your password" },
    { id: "learning", label: "Learning", hint: "Ignore rules" },
  ];

  let canEditSettings = $derived($isOwner);

  let loading = $state(true);
  let error = $state("");
  let activeTab = $state("llm");
  let settingsFilter = $state("");

  let llmProvider = $state("openrouter");
  let llmKey = $state("");
  let llmModel = $state("");
  let llmModelCheap = $state("");
  let llmBaseUrl = $state("");
  let llmDailyBudget = $state("");
  let llmSaving = $state(false);
  let llmMsg = $state("");

  let detectorToggles = $state({});
  let defaultSeverity = $state("warning");
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
  let reviewSaving = $state(false);
  let reviewMsg = $state("");

  let models = $state([]);
  let modelSearch = $state("");
  let modelDropdown = $state(false);
  let cheapModelSearch = $state("");
  let cheapModelDropdown = $state(false);
  let modelFiltered = $derived.by(() => filterModels(modelSearch));
  let cheapModelFiltered = $derived.by(() => filterModels(cheapModelSearch));

  const OPENROUTER_BASE = "https://openrouter.ai/api/v1";

  const DETECTOR_KEYS = [
    "hallucinated_imports", "phantom_deps", "vulnerabilities", "secrets",
    "over_engineering", "boilerplate", "todo_leaks", "stale_api", "risky_patterns", "graph", "guidelines", "iac",
  ];

  const DETECTOR_GROUPS = [
    {
      id: "safety",
      title: "Safety",
      desc: "High-confidence checks. Leave these on unless you have a reason.",
      items: [
        { key: "hallucinated_imports", label: "Fake imports", blurb: "Imports that do not resolve in the registry" },
        { key: "phantom_deps", label: "Missing packages", blurb: "Code uses a package not listed in the manifest" },
        { key: "vulnerabilities", label: "Vulnerabilities", blurb: "Known CVEs via OSV for changed dependencies" },
        { key: "secrets", label: "Secrets", blurb: "API keys, tokens, and other credentials in the diff" },
        { key: "risky_patterns", label: "Risky patterns", blurb: "Dangerous APIs and unsafe defaults" },
      ],
    },
    {
      id: "quality",
      title: "Quality",
      desc: "Maintainability and hygiene. Tune these if reviews feel noisy.",
      items: [
        { key: "over_engineering", label: "Over-engineering", blurb: "Unnecessary abstraction for the change size" },
        { key: "boilerplate", label: "Boilerplate", blurb: "Copy-paste or generated filler" },
        { key: "todo_leaks", label: "TODO leaks", blurb: "TODOs and FIXMEs introduced in the PR" },
        { key: "stale_api", label: "Stale APIs", blurb: "Deprecated or removed API usage" },
        { key: "guidelines", label: "Guidelines", blurb: "Repo guideline / AGENTS.md mismatches" },
      ],
    },
    {
      id: "advanced",
      title: "Advanced",
      desc: "Heavier or niche detectors.",
      items: [
        { key: "graph", label: "Call graph", blurb: "Cross-file impact hints (more compute)" },
        { key: "iac", label: "IaC", blurb: "Terraform / infra misconfigurations" },
      ],
    },
  ];

  const STRICTNESS_OPTIONS = [
    { id: "lenient", label: "Lenient", blurb: "Only high-confidence, merge-blocking issues" },
    { id: "balanced", label: "Balanced", blurb: "Clear bugs and risks; skip preference nits" },
    { id: "strict", label: "Strict", blurb: "Thorough on correctness, security, maintainability" },
    { id: "nitpick", label: "Nitpick", blurb: "Also style, naming, and small clarity notes" },
  ];

  let detectorsOn = $derived(
    DETECTOR_KEYS.filter((k) => detectorToggles[k]).length,
  );

  function setDetectorGroup(groupId, enabled) {
    if (!canEditSettings) return;
    const group = DETECTOR_GROUPS.find((g) => g.id === groupId);
    if (!group) return;
    for (const item of group.items) {
      detectorToggles[item.key] = enabled;
    }
  }

  function setDetectorPreset(preset) {
    if (!canEditSettings) return;
    if (preset === "off") {
      for (const key of DETECTOR_KEYS) detectorToggles[key] = false;
      return;
    }
    if (preset === "safety") {
      for (const key of DETECTOR_KEYS) detectorToggles[key] = false;
      for (const item of DETECTOR_GROUPS[0].items) detectorToggles[item.key] = true;
      return;
    }
    if (preset === "recommended") {
      for (const key of DETECTOR_KEYS) detectorToggles[key] = false;
      for (const item of [...DETECTOR_GROUPS[0].items, ...DETECTOR_GROUPS[1].items]) {
        detectorToggles[item.key] = true;
      }
      return;
    }
    for (const key of DETECTOR_KEYS) detectorToggles[key] = true;
  }

  let learnedRules = $state([]);
  let rulesMsg = $state("");
  let rulesPage = $state(1);

  let rulesPages = $derived(Math.max(1, Math.ceil(learnedRules.length / RULES_PAGE_SIZE)));
  let rulesPageSafe = $derived(Math.min(Math.max(1, rulesPage), rulesPages));
  let pageRules = $derived.by(() => {
    const start = (rulesPageSafe - 1) * RULES_PAGE_SIZE;
    return learnedRules.slice(start, start + RULES_PAGE_SIZE);
  });

  let github = $state(null);
  let githubMsg = $state("");
  let clearingGithub = $state(false);
  let confirmClearGithub = $state(false);
  let testingGithub = $state(false);

  let pwCurrent = $state("");
  let pwNew = $state("");
  let pwMsg = $state("");
  let pwSaving = $state(false);

  // System essentials
  let publicUrl = $state("");
  let auditRetentionDays = $state("90");
  let offlineMode = $state(false);
  let offlineModeSource = $state("off"); // env | db | off
  // Advanced (hidden by default)
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
  let systemSaving = $state(false);
  let systemMsg = $state("");

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
  let testingOidc = $state(false);
  let clearingOidc = $state(false);
  let confirmClearOidc = $state(false);

  // Integrations
  let jiraBaseUrl = $state("");
  let jiraEmail = $state("");
  let jiraApiToken = $state("");
  let linearApiKey = $state("");
  let integrationsSaving = $state(false);
  let integrationsMsg = $state("");
  let testingJira = $state(false);
  let testingLinear = $state(false);
  let clearingTickets = $state(false);
  let confirmClearTickets = $state(false);

  const PROVIDER_DEFAULTS = {
    openrouter: { model: "openai/gpt-4o", baseUrl: OPENROUTER_BASE },
    ollama: { model: "llama3", baseUrl: "http://localhost:11434/v1" },
    custom: { model: "", baseUrl: "" },
    disabled: { model: "", baseUrl: "" },
  };

  const TAB_KEYWORDS = {
    llm: "llm model openrouter ollama budget api key provider",
    review: "review detectors policy strictness severity labels check run autofix warnings blocking paths",
    connections: "github oidc sso jira linear ticket install app auth identity integrations",
    system: "system public url retention offline workers concurrency hsts metrics cookies advanced runtime",
    account: "account password profile",
    learning: "learning rules dismiss ignore",
  };

  let visibleTabs = $derived.by(() => {
    const q = settingsFilter.trim().toLowerCase();
    if (!q) return TABS;
    return TABS.filter(
      (t) =>
        t.label.toLowerCase().includes(q) ||
        (TAB_KEYWORDS[t.id] || "").includes(q)
    );
  });

  function filterModels(q) {
    if (!q) return models.slice(0, 20);
    const needle = q.toLowerCase();
    return models
      .filter((m) => m.id.toLowerCase().includes(needle) || m.name.toLowerCase().includes(needle))
      .slice(0, 15);
  }

  function truthy(v) {
    return ["true", "1", "yes", "on"].includes(String(v ?? "").toLowerCase());
  }

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

  function tabPath(id) {
    switch (id) {
      case "connections":
        return "/app/settings/github";
      case "system":
        return "/app/settings/system";
      case "learning":
        return "/app/settings/learning";
      case "review":
        return "/app/settings/review";
      case "account":
        return "/app/settings/account";
      default:
        return "/app/settings/llm";
    }
  }

  function selectTab(id) {
    if (!TABS.some((t) => t.id === id)) return;
    activeTab = id;
    const next = tabPath(id);
    if (($location || "") !== next) push(next);
  }

  function applyDeepLink() {
    const loc = $location || "";
    if (loc.includes("/settings/github") || loc.includes("/settings/oidc") || loc.includes("/settings/auth")) {
      activeTab = "connections";
    } else if (loc.includes("/settings/runtime") || loc.includes("/settings/system")) {
      activeTab = "system";
    } else if (loc.includes("/settings/learning")) {
      activeTab = "learning";
    } else if (loc.includes("/settings/review")) {
      activeTab = "review";
    } else if (loc.includes("/settings/account")) {
      activeTab = "account";
    } else if (loc.includes("/settings/llm") || loc.endsWith("/settings")) {
      activeTab = "llm";
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
        github = gh;
        llmProvider = data.llm_provider ?? "openrouter";
        llmKey = data.openrouter_api_key ?? "";
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
        offlineMode = truthy(data.offline_mode_effective ?? data.offline_mode);
        offlineModeSource = data.offline_mode_source ?? (offlineMode ? "db" : "off");
        publicUrl = data.public_url ?? "";
        auditRetentionDays = data.audit_retention_days ?? "90";
        queueWorkers = data.queue_workers ?? "";
        maxConcurrentReviews = data.max_concurrent_reviews ?? "";
        hsts = truthy(data.hsts);
        metricsToken = data.metrics_token ?? "";
        reviewTimeoutSecs = data.review_timeout_secs ?? "300";
        maxInlineComments = data.max_inline_comments ?? "8";
        maxReviewerFiles = data.max_reviewer_files ?? "8";
        maxCommentBytes = data.max_comment_bytes ?? "64000";
        maxLlmDiffChars = data.max_llm_diff_chars ?? "8000";
        autoImproveMaxFiles = data.auto_improve_max_files ?? "40";
        autoImproveMaxDiff = data.auto_improve_max_diff ?? "24000";
        allowLocalLlm = truthy(data.allow_local_llm);
        insecureCookies = truthy(data.insecure_cookies);
        oidcIssuer = data.oidc_issuer ?? "";
        oidcClientId = data.oidc_client_id ?? "";
        oidcClientSecret = data.oidc_client_secret ?? "";
        oidcRedirectUri = data.oidc_redirect_uri ?? "";
        oidcScopes = data.oidc_scopes ?? "openid email profile";
        oidcAllowOpenJoin = truthy(data.oidc_allow_open_join);
        oidcAllowUnverifiedEmail = truthy(data.oidc_allow_unverified_email);
        oidcAllowPublicClient = truthy(data.oidc_allow_public_client);
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

  $effect(() => {
    const _loc = $location;
    if (!loading) applyDeepLink();
  });

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

  async function saveReview() {
    reviewSaving = true;
    reviewMsg = "";
    try {
      const pairs = [
        ...Object.entries(detectorToggles).map(([key, enabled]) => [
          `${key}_enabled`,
          enabled ? "true" : "false",
        ]),
        ["default_severity", defaultSeverity],
        ["review_strictness", reviewStrictness],
        ["max_warnings", maxWarnings],
        ["max_blocking", maxBlocking],
        ["forbidden_paths", forbiddenPaths],
        ["auto_labels_enabled", boolVal(autoLabels)],
        ["request_reviewers", boolVal(requestReviewers)],
        ["create_check_run", boolVal(createCheckRun)],
        ["exclude_patterns", excludePatterns],
        ["custom_instructions", customInstructions],
        ["update_pr_description", boolVal(updatePrDescription)],
        ["allow_auto_fix", boolVal(allowAutoFix)],
      ];
      const { failed } = await putMany(pairs);
      reviewMsg = failed === 0 ? "Saved" : `Save failed (${failed} errors)`;
    } catch (err) {
      reviewMsg = err.message || "Save failed";
    } finally {
      reviewSaving = false;
    }
  }

  async function saveSystem() {
    systemSaving = true;
    systemMsg = "";
    try {
      const pairs = [
        ["public_url", publicUrl],
        ["audit_retention_days", auditRetentionDays],
        ["offline_mode", boolVal(offlineMode)],
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
        // Server prefers secure_cookies; keep both flags consistent with the UI toggle.
        ["secure_cookies", boolVal(!insecureCookies)],
      ];
      const { failed, restart } = await putMany(pairs);
      if (!metricsToken.includes("•") && !metricsToken.includes("*")) {
        await api.put("/api/settings/metrics_token", { value: metricsToken.trim() });
      }
      if (failed > 0) systemMsg = `Save failed (${failed} errors)`;
      else if (restart) systemMsg = "Saved. Restart the process to apply worker or concurrency changes.";
      else systemMsg = "Saved";
    } catch (err) {
      systemMsg = err.message || "Save failed";
    } finally {
      systemSaving = false;
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

  async function deleteRule(id) {
    rulesMsg = "";
    try {
      await api.delete(`/api/learning/rules/${id}`);
      learnedRules = learnedRules.filter((r) => r.id !== id);
      if (rulesPage > 1 && (rulesPage - 1) * RULES_PAGE_SIZE >= learnedRules.length) {
        rulesPage = Math.max(1, rulesPage - 1);
      }
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

  async function testGithub() {
    testingGithub = true;
    githubMsg = "";
    try {
      const res = await api.post("/api/settings/github/test", {});
      githubMsg = res?.message || "GitHub App connection OK";
    } catch (err) {
      githubMsg = err.message || "GitHub test failed";
    } finally {
      testingGithub = false;
    }
  }

  async function testOidc() {
    testingOidc = true;
    authMsg = "";
    try {
      // Persist form values first so the test uses what the user typed.
      await saveAuth();
      if (authMsg && authMsg !== "Saved") return;
      const res = await api.post("/api/settings/oidc/test", {});
      authMsg = res?.message || "OIDC discovery OK";
    } catch (err) {
      authMsg = err.message || "OIDC test failed";
    } finally {
      testingOidc = false;
    }
  }

  async function clearOidc() {
    clearingOidc = true;
    authMsg = "";
    try {
      const res = await api.delete("/api/settings/oidc");
      oidcIssuer = "";
      oidcClientId = "";
      oidcClientSecret = "";
      oidcRedirectUri = "";
      oidcScopes = "openid email profile";
      oidcAllowOpenJoin = false;
      oidcAllowUnverifiedEmail = false;
      oidcAllowPublicClient = false;
      confirmClearOidc = false;
      authMsg = res?.message || "SSO cleared";
    } catch (err) {
      authMsg = err.message || "Clear failed";
    } finally {
      clearingOidc = false;
    }
  }

  async function testJira() {
    testingJira = true;
    integrationsMsg = "";
    try {
      await saveIntegrations();
      if (integrationsMsg && integrationsMsg !== "Saved") return;
      const res = await api.post("/api/settings/jira/test", {});
      integrationsMsg = res?.message || "Jira connection OK";
    } catch (err) {
      integrationsMsg = err.message || "Jira test failed";
    } finally {
      testingJira = false;
    }
  }

  async function testLinear() {
    testingLinear = true;
    integrationsMsg = "";
    try {
      await saveIntegrations();
      if (integrationsMsg && integrationsMsg !== "Saved") return;
      const res = await api.post("/api/settings/linear/test", {});
      integrationsMsg = res?.message || "Linear connection OK";
    } catch (err) {
      integrationsMsg = err.message || "Linear test failed";
    } finally {
      testingLinear = false;
    }
  }

  async function clearTickets() {
    clearingTickets = true;
    integrationsMsg = "";
    try {
      const res = await api.delete("/api/settings/tickets");
      jiraBaseUrl = "";
      jiraEmail = "";
      jiraApiToken = "";
      linearApiKey = "";
      confirmClearTickets = false;
      integrationsMsg = res?.message || "Ticket integrations cleared";
    } catch (err) {
      integrationsMsg = err.message || "Clear failed";
    } finally {
      clearingTickets = false;
    }
  }

  function toggleFlag(setter, current) {
    if (!canEditSettings) return;
    setter(!current);
  }
</script>

<AppShell title="Settings">
  <LoadingSpinner loading={loading} />
  {#if error}
    <ErrorState message={error} />
  {:else if loading}
  {:else}
    <div class="settings-shell">
      <div class="settings-page-head">
        <div>
          <h1 class="page-title">Settings</h1>
          <p class="page-description">Pick a category on the left. Search finds sections fast.</p>
        </div>
        <label class="settings-filter" for="settings-filter">
          <span class="sr-only">Search settings</span>
          <input
            id="settings-filter"
            type="search"
            placeholder="Search settings…"
            bind:value={settingsFilter}
            autocomplete="off"
          />
        </label>
      </div>

      {#if !canEditSettings}
        <div class="settings-readonly-banner" role="status">
          Only owners can change org settings. Your role: {roleLabel($currentUser?.role, $currentUser?.is_bootstrap)}.
        </div>
      {/if}

      {#if visibleTabs.length === 0}
        <p class="empty-note">No sections match “{settingsFilter}”.</p>
      {:else}
        <div class="settings-body">
          <nav class="settings-side-nav" aria-label="Settings categories">
            {#each visibleTabs as s}
              <button
                type="button"
                class="settings-side-item"
                class:active={activeTab === s.id}
                aria-current={activeTab === s.id ? "page" : undefined}
                onclick={() => selectTab(s.id)}
              >
                <span class="settings-side-label">{s.label}</span>
                <span class="settings-side-hint">{s.hint}</span>
              </button>
            {/each}
          </nav>

          <div class="settings-main">
            {#if !visibleTabs.some((t) => t.id === activeTab)}
              <p class="empty-note">
                Open
                {#each visibleTabs as t, i}
                  {#if i > 0}, {/if}<button type="button" class="quiet sm" onclick={() => selectTab(t.id)}>{t.label}</button>
                {/each}.
              </p>
            {:else}
              <div
                class="settings-panel"
                role="region"
                id="panel-{activeTab}"
                aria-label={TABS.find((t) => t.id === activeTab)?.label || "Settings"}
              >
      {#if activeTab === "llm"}
        <section class="card settings-card">
          <header class="settings-section-head">
            <h3 class="section-heading">LLM</h3>
            <p class="section-desc">Optional. Tier-1 detectors run without a model.</p>
          </header>
          <div class="form-group">
            <label for="llm-provider">Provider</label>
            <select id="llm-provider" bind:value={llmProvider} onchange={(e) => handleProviderChange(e.target.value)} disabled={!canEditSettings}>
              <option value="openrouter">OpenRouter</option>
              <option value="ollama">Ollama</option>
              <option value="custom">Custom</option>
              <option value="disabled">Disabled</option>
            </select>
          </div>
          {#if llmProvider !== "disabled"}
            <div class="form-group">
              <label for="llm-key">API Key</label>
              <input id="llm-key" type="password" bind:value={llmKey} placeholder="sk-..." disabled={!canEditSettings} />
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
                      placeholder="Search models…" autocomplete="off" disabled={!canEditSettings} />
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
                  <input id="llm-model" type="text" bind:value={llmModel} disabled={!canEditSettings} />
                {/if}
              </div>
              <div class="form-group model-search">
                <label for="llm-model-cheap">Cheap model</label>
                {#if llmProvider === "openrouter"}
                  <div class="search-wrap">
                    <input id="llm-model-cheap" type="text" bind:value={cheapModelSearch}
                      oninput={() => (cheapModelDropdown = true)}
                      onfocus={() => (cheapModelDropdown = true)}
                      onkeydown={handleCheapModelKeydown}
                      onblur={handleCheapModelBlur}
                      placeholder="Search models…" autocomplete="off" disabled={!canEditSettings} />
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
                    placeholder="optional" disabled={!canEditSettings} />
                {/if}
              </div>
            </div>
            <div class="form-group">
              <label for="llm-url">Base URL</label>
              {#if llmProvider === "openrouter"}
                <input id="llm-url" type="text" value={OPENROUTER_BASE} readonly class="input-readonly" />
              {:else}
                <input id="llm-url" type="text" bind:value={llmBaseUrl} placeholder="https://…" disabled={!canEditSettings} />
              {/if}
            </div>
            <div class="form-group">
              <label for="llm-budget">Daily budget USD (0 = unlimited)</label>
              <input id="llm-budget" type="text" bind:value={llmDailyBudget} placeholder="e.g. 5" disabled={!canEditSettings} />
            </div>
          {/if}
          <div class="save-row">
            <button onclick={saveLLM} disabled={llmSaving || !canEditSettings}>{llmSaving ? "Saving…" : "Save"}</button>
            {#if llmMsg}<span class="save-msg" class:error={llmMsg !== "Saved"}>{llmMsg}</span>{/if}
          </div>
        </section>

      {:else if activeTab === "review"}
        <div class="settings-stack review-stack">
          <section class="card settings-card">
            <header class="settings-section-head review-head">
              <div>
                <h3 class="section-heading">Review</h3>
                <p class="section-desc">What Codasaurus checks on PRs, and how loud it is.</p>
              </div>
              <span class="review-count">{detectorsOn} of {DETECTOR_KEYS.length} detectors on</span>
            </header>

            <div class="review-presets" role="group" aria-label="Detector presets">
              <button type="button" class="chip" disabled={!canEditSettings} onclick={() => setDetectorPreset("recommended")}>Recommended</button>
              <button type="button" class="chip" disabled={!canEditSettings} onclick={() => setDetectorPreset("safety")}>Safety only</button>
              <button type="button" class="chip" disabled={!canEditSettings} onclick={() => setDetectorPreset("all")}>All on</button>
              <button type="button" class="chip" disabled={!canEditSettings} onclick={() => setDetectorPreset("off")}>All off</button>
            </div>

            {#each DETECTOR_GROUPS as group}
              <div class="detector-group">
                <div class="detector-group-head">
                  <div>
                    <h4 class="settings-subhead tight">{group.title}</h4>
                    <p class="group-desc">{group.desc}</p>
                  </div>
                  <div class="group-actions">
                    <button type="button" class="quiet sm" disabled={!canEditSettings} onclick={() => setDetectorGroup(group.id, true)}>All on</button>
                    <button type="button" class="quiet sm" disabled={!canEditSettings} onclick={() => setDetectorGroup(group.id, false)}>All off</button>
                  </div>
                </div>
                <div class="detector-cards">
                  {#each group.items as item}
                    <div class="detector-card" class:on={detectorToggles[item.key]}>
                      <div class="detector-card-text">
                        <span class="detector-card-label">{item.label}</span>
                        <span class="detector-card-blurb">{item.blurb}</span>
                      </div>
                      <label class="toggle">
                        <div
                          class="toggle-track"
                          class:on={detectorToggles[item.key] ?? false}
                          role="checkbox"
                          aria-checked={detectorToggles[item.key] ?? false}
                          aria-label={item.label}
                          tabindex="0"
                          onclick={() =>
                            canEditSettings &&
                            (detectorToggles[item.key] = !(detectorToggles[item.key] ?? false))}
                          onkeydown={(e) => {
                            if (canEditSettings && e.key === "Enter")
                              detectorToggles[item.key] = !(detectorToggles[item.key] ?? false);
                          }}
                        >
                          <div class="toggle-knob"></div>
                        </div>
                      </label>
                    </div>
                  {/each}
                </div>
              </div>
            {/each}
          </section>

          <section class="card settings-card">
            <header class="settings-section-head">
              <h3 class="section-heading">Tone &amp; thresholds</h3>
              <p class="section-desc">How many findings to surface, and how strict the bar is.</p>
            </header>

            <h4 class="settings-subhead tight">Strictness</h4>
            <div class="strictness-grid" role="radiogroup" aria-label="Review strictness">
              {#each STRICTNESS_OPTIONS as opt}
                <button
                  type="button"
                  class="choice"
                  class:active={reviewStrictness === opt.id}
                  role="radio"
                  aria-checked={reviewStrictness === opt.id}
                  disabled={!canEditSettings}
                  onclick={() => (reviewStrictness = opt.id)}
                >
                  <strong>{opt.label}</strong>
                  <span>{opt.blurb}</span>
                </button>
              {/each}
            </div>

            <div class="form-row-2" style="margin-top: 20px">
              <div class="form-group">
                <label for="default-severity">Minimum severity posted</label>
                <select id="default-severity" bind:value={defaultSeverity} disabled={!canEditSettings}>
                  <option value="blocking">Blocking only</option>
                  <option value="warning">Warning and above</option>
                  <option value="info">Include info</option>
                </select>
                <p class="field-hint">Findings below this level stay off the PR.</p>
              </div>
              <div class="form-group">
                <label for="max-warnings">Warning budget</label>
                <input id="max-warnings" type="number" min="0" bind:value={maxWarnings} disabled={!canEditSettings} />
                <p class="field-hint">Cap on warning-level comments per review.</p>
              </div>
            </div>
            <div class="form-group">
              <label for="max-blocking">Blocking budget</label>
              <input id="max-blocking" type="number" min="0" bind:value={maxBlocking} disabled={!canEditSettings} />
              <p class="field-hint">0 means no extra cap beyond detector output.</p>
            </div>
            <div class="form-group">
              <label for="custom-instructions">Custom instructions</label>
              <textarea
                id="custom-instructions"
                rows="3"
                bind:value={customInstructions}
                placeholder="e.g. Prefer small PRs. Ignore generated protobuf files."
                disabled={!canEditSettings}
              ></textarea>
              <p class="field-hint">Appended to the reviewer prompt for every PR.</p>
            </div>
          </section>

          <section class="card settings-card">
            <header class="settings-section-head">
              <h3 class="section-heading">PR actions</h3>
              <p class="section-desc">What happens on GitHub besides inline comments.</p>
            </header>
            <div class="action-list">
              <div class="action-row">
                <div>
                  <span class="action-label">Auto-apply labels</span>
                  <span class="action-blurb">Tag PRs from review outcome (e.g. risk labels).</span>
                </div>
                <label class="toggle">
                  <div class="toggle-track" class:on={autoLabels} role="checkbox" aria-checked={autoLabels}
                    tabindex="0"
                    onclick={() => toggleFlag((v) => (autoLabels = v), autoLabels)}
                    onkeydown={(e) => { if (e.key === 'Enter') toggleFlag((v) => (autoLabels = v), autoLabels); }}>
                    <div class="toggle-knob"></div>
                  </div>
                </label>
              </div>
              <div class="action-row">
                <div>
                  <span class="action-label">Request CODEOWNERS</span>
                  <span class="action-blurb">Ask owners to review when the bot finishes.</span>
                </div>
                <label class="toggle">
                  <div class="toggle-track" class:on={requestReviewers} role="checkbox" aria-checked={requestReviewers}
                    tabindex="0"
                    onclick={() => toggleFlag((v) => (requestReviewers = v), requestReviewers)}
                    onkeydown={(e) => { if (e.key === 'Enter') toggleFlag((v) => (requestReviewers = v), requestReviewers); }}>
                    <div class="toggle-knob"></div>
                  </div>
                </label>
              </div>
              <div class="action-row">
                <div>
                  <span class="action-label">Check runs</span>
                  <span class="action-blurb">Show a GitHub check with pass/fail status.</span>
                </div>
                <label class="toggle">
                  <div class="toggle-track" class:on={createCheckRun} role="checkbox" aria-checked={createCheckRun}
                    tabindex="0"
                    onclick={() => toggleFlag((v) => (createCheckRun = v), createCheckRun)}
                    onkeydown={(e) => { if (e.key === 'Enter') toggleFlag((v) => (createCheckRun = v), createCheckRun); }}>
                    <div class="toggle-knob"></div>
                  </div>
                </label>
              </div>
            </div>

            <details class="settings-advanced">
              <summary>Paths, describe, and autofix</summary>
              <div class="form-group">
                <label for="forbidden-paths">Forbidden path prefixes</label>
                <input id="forbidden-paths" type="text" bind:value={forbiddenPaths} placeholder="vendor/,secrets/" disabled={!canEditSettings} />
                <p class="field-hint">Touching these paths can fail the review.</p>
              </div>
              <div class="form-group">
                <label for="exclude-patterns">Exclude path patterns</label>
                <input id="exclude-patterns" type="text" bind:value={excludePatterns} placeholder="vendor/,*.lock,dist/" disabled={!canEditSettings} />
                <p class="field-hint">Skip these files when scanning the diff.</p>
              </div>
              <div class="action-list">
                <div class="action-row">
                  <div>
                    <span class="action-label">Update PR body on describe</span>
                    <span class="action-blurb">Slash describe can rewrite the PR description.</span>
                  </div>
                  <label class="toggle">
                    <div class="toggle-track" class:on={updatePrDescription} role="checkbox" aria-checked={updatePrDescription}
                      tabindex="0"
                      onclick={() => toggleFlag((v) => (updatePrDescription = v), updatePrDescription)}
                      onkeydown={(e) => { if (e.key === 'Enter') toggleFlag((v) => (updatePrDescription = v), updatePrDescription); }}>
                      <div class="toggle-knob"></div>
                    </div>
                  </label>
                </div>
                <div class="action-row">
                  <div>
                    <span class="action-label">Allow @codasaurus fix</span>
                    <span class="action-blurb">Lets the bot push autofix commits when asked.</span>
                  </div>
                  <label class="toggle">
                    <div class="toggle-track" class:on={allowAutoFix} role="checkbox" aria-checked={allowAutoFix}
                      tabindex="0"
                      onclick={() => toggleFlag((v) => (allowAutoFix = v), allowAutoFix)}
                      onkeydown={(e) => { if (e.key === 'Enter') toggleFlag((v) => (allowAutoFix = v), allowAutoFix); }}>
                      <div class="toggle-knob"></div>
                    </div>
                  </label>
                </div>
              </div>
            </details>
          </section>

          <div class="review-save-bar">
            <button class="primary" onclick={saveReview} disabled={reviewSaving || !canEditSettings}>
              {reviewSaving ? "Saving…" : "Save review settings"}
            </button>
            {#if reviewMsg}<span class="save-msg" class:error={reviewMsg !== "Saved"}>{reviewMsg}</span>{/if}
          </div>
        </div>

      {:else if activeTab === "connections"}
        <div class="settings-stack">
          <section class="card settings-card" id="github-connection">
            <header class="settings-section-head">
              <h3 class="section-heading">GitHub App</h3>
              <p class="section-desc">Install URL and local credentials.</p>
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
                <button type="button" onclick={testGithub} disabled={testingGithub}>
                  {testingGithub ? "Testing…" : "Test connection"}
                </button>
                <button type="button" onclick={openInstallUrl}>Open install URL</button>
                <a
                  class="btn sm"
                  href="https://github.com/settings/apps"
                  target="_blank"
                  rel="noopener noreferrer"
                >Manage on GitHub ↗</a>
              </div>
              {#if githubMsg}
                <p class="save-msg" class:error={/fail|error|reject/i.test(githubMsg)}>{githubMsg}</p>
              {/if}
              <div class="danger-zone" style="margin-top:16px">
                <h3>Danger zone</h3>
                <p>Clears local App credentials and marks repos inactive. Does not uninstall on GitHub.</p>
                {#if !confirmClearGithub}
                  <button class="danger" onclick={() => (confirmClearGithub = true)} disabled={!canEditSettings}>Clear local GitHub config</button>
                {:else}
                  <div style="display:flex;gap:8px;margin-top:8px">
                    <button class="danger" onclick={clearLocalGithub} disabled={clearingGithub}>
                      {clearingGithub ? "Clearing…" : "Confirm clear"}
                    </button>
                    <button onclick={() => (confirmClearGithub = false)}>Cancel</button>
                  </div>
                {/if}
              </div>
            {:else}
              <p class="empty-note">No GitHub App configured yet.</p>
              {#if githubMsg}
                <p class="save-msg" class:error={/fail|error/i.test(githubMsg)}>{githubMsg}</p>
              {/if}
              <div class="save-row">
                <button class="primary" onclick={openInstallUrl}>Install GitHub App</button>
              </div>
            {/if}
          </section>

          <section class="card settings-card">
            <header class="settings-section-head">
              <h3 class="section-heading">SSO (OIDC)</h3>
              <p class="section-desc">Optional identity provider for dashboard login.</p>
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
            <details class="settings-advanced">
              <summary>SSO advanced</summary>
              <div class="form-group">
                <label for="oidc-redirect">Redirect URI</label>
                <input id="oidc-redirect" type="url" bind:value={oidcRedirectUri} placeholder="Defaults from Public URL" disabled={!canEditSettings} />
              </div>
              <div class="form-group">
                <label for="oidc-scopes">Scopes</label>
                <input id="oidc-scopes" type="text" bind:value={oidcScopes} disabled={!canEditSettings} />
              </div>
              <div class="toggle-stack">
                <div class="detector-row">
                  <span>Allow open join (no invite)</span>
                  <label class="toggle">
                    <div class="toggle-track" class:on={oidcAllowOpenJoin} role="checkbox" aria-checked={oidcAllowOpenJoin}
                      tabindex="0"
                      onclick={() => toggleFlag((v) => (oidcAllowOpenJoin = v), oidcAllowOpenJoin)}
                      onkeydown={(e) => { if (e.key === 'Enter') toggleFlag((v) => (oidcAllowOpenJoin = v), oidcAllowOpenJoin); }}>
                      <div class="toggle-knob"></div>
                    </div>
                  </label>
                </div>
                <div class="detector-row">
                  <span>Allow unverified email</span>
                  <label class="toggle">
                    <div class="toggle-track" class:on={oidcAllowUnverifiedEmail} role="checkbox" aria-checked={oidcAllowUnverifiedEmail}
                      tabindex="0"
                      onclick={() => toggleFlag((v) => (oidcAllowUnverifiedEmail = v), oidcAllowUnverifiedEmail)}
                      onkeydown={(e) => { if (e.key === 'Enter') toggleFlag((v) => (oidcAllowUnverifiedEmail = v), oidcAllowUnverifiedEmail); }}>
                      <div class="toggle-knob"></div>
                    </div>
                  </label>
                </div>
                <div class="detector-row">
                  <span>Allow public client</span>
                  <label class="toggle">
                    <div class="toggle-track" class:on={oidcAllowPublicClient} role="checkbox" aria-checked={oidcAllowPublicClient}
                      tabindex="0"
                      onclick={() => toggleFlag((v) => (oidcAllowPublicClient = v), oidcAllowPublicClient)}
                      onkeydown={(e) => { if (e.key === 'Enter') toggleFlag((v) => (oidcAllowPublicClient = v), oidcAllowPublicClient); }}>
                      <div class="toggle-knob"></div>
                    </div>
                  </label>
                </div>
              </div>
            </details>
            <div class="save-row">
              <button onclick={saveAuth} disabled={authSaving || !canEditSettings}>{authSaving ? "Saving…" : "Save SSO"}</button>
              <button type="button" onclick={testOidc} disabled={testingOidc || authSaving || !canEditSettings || !oidcIssuer}>
                {testingOidc ? "Testing…" : "Test discovery"}
              </button>
              {#if authMsg}<span class="save-msg" class:error={/fail|error|reject|missing|invalid/i.test(authMsg)}>{authMsg}</span>{/if}
            </div>
            {#if canEditSettings && (oidcIssuer || oidcClientId)}
              <div class="danger-zone" style="margin-top:16px">
                <h3>Clear SSO</h3>
                <p>Removes OIDC settings from this instance (IdP app is unchanged).</p>
                {#if !confirmClearOidc}
                  <button class="danger" onclick={() => (confirmClearOidc = true)}>Clear SSO config</button>
                {:else}
                  <div style="display:flex;gap:8px;margin-top:8px">
                    <button class="danger" onclick={clearOidc} disabled={clearingOidc}>
                      {clearingOidc ? "Clearing…" : "Confirm clear"}
                    </button>
                    <button onclick={() => (confirmClearOidc = false)}>Cancel</button>
                  </div>
                {/if}
              </div>
            {/if}
          </section>

          <section class="card settings-card">
            <header class="settings-section-head">
              <h3 class="section-heading">Tickets</h3>
              <p class="section-desc">Optional Jira / Linear context from PR title and body (keys like ENG-123 or Linear issue URLs).</p>
            </header>
            <div class="form-group">
              <label for="jira-base">Jira base URL</label>
              <input id="jira-base" type="url" bind:value={jiraBaseUrl} placeholder="https://org.atlassian.net" disabled={!canEditSettings} />
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
              <button onclick={saveIntegrations} disabled={integrationsSaving || !canEditSettings}>{integrationsSaving ? "Saving…" : "Save tickets"}</button>
              <button type="button" onclick={testJira} disabled={testingJira || integrationsSaving || !canEditSettings || !jiraBaseUrl}>
                {testingJira ? "Testing…" : "Test Jira"}
              </button>
              <button type="button" onclick={testLinear} disabled={testingLinear || integrationsSaving || !canEditSettings || !linearApiKey}>
                {testingLinear ? "Testing…" : "Test Linear"}
              </button>
              {#if integrationsMsg}<span class="save-msg" class:error={/fail|error|reject|missing|invalid/i.test(integrationsMsg)}>{integrationsMsg}</span>{/if}
            </div>
            {#if canEditSettings && (jiraBaseUrl || jiraEmail || linearApiKey)}
              <div class="danger-zone" style="margin-top:16px">
                <h3>Clear tickets</h3>
                <p>Removes Jira and Linear credentials from this instance.</p>
                {#if !confirmClearTickets}
                  <button class="danger" onclick={() => (confirmClearTickets = true)}>Clear ticket config</button>
                {:else}
                  <div style="display:flex;gap:8px;margin-top:8px">
                    <button class="danger" onclick={clearTickets} disabled={clearingTickets}>
                      {clearingTickets ? "Clearing…" : "Confirm clear"}
                    </button>
                    <button onclick={() => (confirmClearTickets = false)}>Cancel</button>
                  </div>
                {/if}
              </div>
            {/if}
          </section>
        </div>

      {:else if activeTab === "system"}
        <section class="card settings-card">
          <header class="settings-section-head">
            <h3 class="section-heading">System</h3>
            <p class="section-desc">Deployment basics. Tuning and security overrides are under Advanced.</p>
          </header>
          <div class="form-group">
            <label for="public-url">Public URL</label>
            <input id="public-url" type="url" bind:value={publicUrl} placeholder="https://reviews.example.com" disabled={!canEditSettings} />
            <p class="field-hint">Used for invites, OIDC redirects, and HTTPS HSTS.</p>
          </div>
          <div class="form-group">
            <label for="audit-retention">Audit retention (days)</label>
            <input id="audit-retention" type="number" min="7" max="730" bind:value={auditRetentionDays} disabled={!canEditSettings} />
          </div>
          <div class="detector-row" style="border:none;padding:8px 0">
            <span>Offline / air-gap mode</span>
            <label class="toggle">
              <div class="toggle-track" class:on={offlineMode} role="checkbox" aria-checked={offlineMode}
                tabindex="0"
                onclick={() => toggleFlag((v) => (offlineMode = v), offlineMode)}
                onkeydown={(e) => { if (e.key === 'Enter') toggleFlag((v) => (offlineMode = v), offlineMode); }}>
                <div class="toggle-knob"></div>
              </div>
            </label>
          </div>
          <p class="field-hint">
            Independent of your OpenRouter key — when on, LLM and live registry/OSV calls are blocked.
            {#if offlineModeSource === "env"}
              Currently forced by the <code>CODASAURUS_OFFLINE</code> environment variable (e.g. Render).
              Remove that env var and redeploy, or turn this off and Save (clears the in-process flag until the next deploy).
            {:else if offlineMode}
              Turn off and Save under System to re-enable LLM reviews.
            {/if}
          </p>

          <details class="settings-advanced">
            <summary>Advanced (workers, caps, security)</summary>
            <p class="field-hint" style="margin-bottom:12px">Defaults are fine for most installs. Workers / concurrency need a process restart.</p>
            <div class="form-row-2">
              <div class="form-group">
                <label for="queue-workers">Queue workers</label>
                <input id="queue-workers" type="number" min="1" max="8" bind:value={queueWorkers} placeholder="auto" disabled={!canEditSettings} />
              </div>
              <div class="form-group">
                <label for="max-concurrent">Max concurrent reviews</label>
                <input id="max-concurrent" type="number" min="1" max="64" bind:value={maxConcurrentReviews} placeholder="4" disabled={!canEditSettings} />
              </div>
            </div>
            <div class="form-row-2">
              <div class="form-group">
                <label for="review-timeout">Review timeout (secs)</label>
                <input id="review-timeout" type="number" bind:value={reviewTimeoutSecs} disabled={!canEditSettings} />
              </div>
              <div class="form-group">
                <label for="max-inline">Max inline comments</label>
                <input id="max-inline" type="number" bind:value={maxInlineComments} disabled={!canEditSettings} />
              </div>
            </div>
            <div class="form-row-2">
              <div class="form-group">
                <label for="max-reviewer-files">Max reviewer files</label>
                <input id="max-reviewer-files" type="number" bind:value={maxReviewerFiles} disabled={!canEditSettings} />
              </div>
              <div class="form-group">
                <label for="max-comment-bytes">Max comment bytes</label>
                <input id="max-comment-bytes" type="number" bind:value={maxCommentBytes} disabled={!canEditSettings} />
              </div>
            </div>
            <div class="form-row-2">
              <div class="form-group">
                <label for="max-llm-diff">Max LLM diff chars</label>
                <input id="max-llm-diff" type="number" bind:value={maxLlmDiffChars} disabled={!canEditSettings} />
              </div>
              <div class="form-group">
                <label for="auto-improve-files">Auto-improve max files</label>
                <input id="auto-improve-files" type="number" bind:value={autoImproveMaxFiles} disabled={!canEditSettings} />
              </div>
            </div>
            <div class="form-group">
              <label for="auto-improve-diff">Auto-improve max diff chars</label>
              <input id="auto-improve-diff" type="number" bind:value={autoImproveMaxDiff} disabled={!canEditSettings} />
            </div>
            <div class="form-group">
              <label for="metrics-token">Metrics bearer token</label>
              <input id="metrics-token" type="password" bind:value={metricsToken} placeholder="Blank disables /metrics" disabled={!canEditSettings} autocomplete="off" />
              <p class="field-hint">Leave blank and save to clear the token and disable <code>/metrics</code>.</p>
            </div>
            <div class="toggle-stack">
              <div class="detector-row">
                <span>Force HSTS</span>
                <label class="toggle">
                  <div class="toggle-track" class:on={hsts} role="checkbox" aria-checked={hsts}
                    tabindex="0"
                    onclick={() => toggleFlag((v) => (hsts = v), hsts)}
                    onkeydown={(e) => { if (e.key === 'Enter') toggleFlag((v) => (hsts = v), hsts); }}>
                    <div class="toggle-knob"></div>
                  </div>
                </label>
              </div>
              <div class="detector-row">
                <span>Allow local LLM endpoints</span>
                <label class="toggle">
                  <div class="toggle-track" class:on={allowLocalLlm} role="checkbox" aria-checked={allowLocalLlm}
                    tabindex="0"
                    onclick={() => toggleFlag((v) => (allowLocalLlm = v), allowLocalLlm)}
                    onkeydown={(e) => { if (e.key === 'Enter') toggleFlag((v) => (allowLocalLlm = v), allowLocalLlm); }}>
                    <div class="toggle-knob"></div>
                  </div>
                </label>
              </div>
              <div class="detector-row">
                <span>Allow insecure cookies (HTTP)</span>
                <label class="toggle">
                  <div class="toggle-track" class:on={insecureCookies} role="checkbox" aria-checked={insecureCookies}
                    tabindex="0"
                    onclick={() => toggleFlag((v) => (insecureCookies = v), insecureCookies)}
                    onkeydown={(e) => { if (e.key === 'Enter') toggleFlag((v) => (insecureCookies = v), insecureCookies); }}>
                    <div class="toggle-knob"></div>
                  </div>
                </label>
              </div>
            </div>
          </details>

          <div class="save-row">
            <button onclick={saveSystem} disabled={systemSaving || !canEditSettings}>{systemSaving ? "Saving…" : "Save"}</button>
            {#if systemMsg}<span class="save-msg" class:error={!systemMsg.startsWith("Saved")}>{systemMsg}</span>{/if}
          </div>
        </section>

      {:else if activeTab === "account"}
        <section class="card settings-card">
          <header class="settings-section-head">
            <h3 class="section-heading">Account</h3>
            <p class="section-desc">
              {roleLabel($currentUser?.role, $currentUser?.is_bootstrap)} · {$currentUser?.email}.
              Members live on <a href="#/app/team">Team</a>.
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
            <p class="empty-note">SSO account. Password is managed by your identity provider.</p>
          {/if}
        </section>

      {:else if activeTab === "learning"}
        <section class="card settings-card">
          <header class="settings-section-head">
            <h3 class="section-heading">Learning</h3>
            <p class="section-desc">Ignore rules taught by dismissing findings.</p>
          </header>
          {#if learnedRules.length === 0}
            <p class="empty-note">No learned ignore rules yet.</p>
          {:else}
            <div class="detector-list">
              {#each pageRules as rule}
                <div class="detector-row">
                  <span>
                    <strong>{rule.detector}</strong>
                    {#if rule.file_pattern} · <code>{rule.file_pattern}</code>{/if}
                    <br /><span class="muted">{rule.reason || rule.action}</span>
                  </span>
                  <button type="button" class="quiet sm" onclick={() => deleteRule(rule.id)} disabled={!$isMaintainer}>Delete</button>
                </div>
              {/each}
            </div>
            {#if learnedRules.length > RULES_PAGE_SIZE}
              <div class="rules-page-meta">
                <span class="muted">
                  {learnedRules.length} rules · page {rulesPageSafe} of {rulesPages}
                </span>
                <Pagination
                  page={rulesPageSafe}
                  totalPages={rulesPages}
                  onChange={(p) => (rulesPage = p)}
                />
              </div>
            {/if}
          {/if}
          {#if rulesMsg}<p class="save-msg" class:error={rulesMsg !== "Rule deleted"}>{rulesMsg}</p>{/if}
        </section>
      {/if}
              </div>
            {/if}
          </div>
        </div>
      {/if}
    </div>
  {/if}
</AppShell>

<style>
  .settings-shell {
    width: 100%;
    max-width: 1120px;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .settings-page-head {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 20px;
    flex-wrap: wrap;
    margin-bottom: 20px;
    flex-shrink: 0;
  }

  .settings-filter input {
    width: min(260px, 100%);
    font-size: 14px;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg-primary);
    color: var(--text-primary);
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    border: 0;
  }

  .settings-body {
    display: grid;
    grid-template-columns: 220px minmax(0, 1fr);
    gap: 28px;
    flex: 1;
    min-height: 0;
    align-items: stretch;
  }

  .settings-side-nav {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 4px;
    border: 1px solid var(--border);
    border-radius: 12px;
    background: color-mix(in srgb, var(--bg-secondary) 45%, transparent);
    align-self: start;
    max-height: 100%;
    overflow-y: auto;
  }

  .settings-side-item {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    width: 100%;
    text-align: left;
    border: none;
    border-radius: 8px;
    background: transparent;
    color: var(--text-secondary);
    padding: 10px 12px;
    cursor: pointer;
  }

  .settings-side-item:hover {
    background: var(--bg-secondary);
    color: var(--text-primary);
  }

  .settings-side-item.active {
    background: var(--bg-tertiary);
    color: var(--text-primary);
    box-shadow: inset 2px 0 var(--accent-soft);
  }

  .settings-side-label {
    font-size: 14px;
    font-weight: 600;
  }

  .settings-side-hint {
    font-size: 12px;
    color: var(--text-muted);
    font-weight: 400;
    line-height: 1.35;
  }

  .settings-side-item.active .settings-side-hint {
    color: var(--text-secondary);
  }

  .settings-main {
    min-width: 0;
    min-height: 0;
    overflow-y: auto;
    padding-right: 4px;
    scrollbar-gutter: stable;
  }

  .settings-panel {
    width: 100%;
    max-width: none;
  }

  .settings-section-head {
    margin-bottom: 20px;
  }

  .settings-subhead {
    margin: 28px 0 14px;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .settings-subhead.tight {
    margin: 0 0 4px;
  }

  .section-desc {
    margin: 6px 0 0;
    font-size: 14px;
    color: var(--text-muted);
    line-height: 1.45;
  }

  .settings-stack {
    display: flex;
    flex-direction: column;
    gap: 20px;
  }

  .settings-advanced {
    margin-top: 20px;
    padding: 16px 18px;
    border: 1px solid var(--border-light);
    border-radius: 10px;
    background: color-mix(in srgb, var(--bg-secondary) 40%, transparent);
  }

  .settings-advanced summary {
    cursor: pointer;
    font-size: 14px;
    font-weight: 500;
    color: var(--text-muted);
    user-select: none;
  }

  .settings-advanced[open] summary {
    margin-bottom: 14px;
    color: var(--text-primary);
  }

  .settings-meta-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 20px;
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
    margin: 0;
  }

  .detector-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 12px 0;
    border-bottom: 1px solid var(--border-light);
    font-size: 14px;
  }

  .detector-row:last-child {
    border-bottom: none;
  }

  .review-stack {
    padding-bottom: 8px;
  }

  .review-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
    flex-wrap: wrap;
  }

  .review-count {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-muted);
    padding: 6px 10px;
    border-radius: 999px;
    border: 1px solid var(--border);
    background: var(--bg-secondary);
    white-space: nowrap;
  }

  .review-presets {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-bottom: 20px;
  }

  .detector-group {
    margin-top: 8px;
    padding-top: 16px;
    border-top: 1px solid var(--border-light);
  }

  .detector-group:first-of-type {
    border-top: none;
    padding-top: 0;
  }

  .detector-group-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 12px;
  }

  .group-desc {
    margin: 0;
    font-size: 13px;
    color: var(--text-muted);
    line-height: 1.4;
  }

  .group-actions {
    display: flex;
    gap: 8px;
    flex-shrink: 0;
  }

  .detector-cards {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
  }

  .detector-card {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    padding: 12px 14px;
    border: 1px solid var(--border-light);
    border-radius: 10px;
    background: color-mix(in srgb, var(--bg-secondary) 35%, transparent);
  }

  .detector-card.on {
    border-color: color-mix(in srgb, var(--accent-soft) 35%, var(--border));
  }

  .detector-card-text {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }

  .detector-card-label {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .detector-card-blurb {
    font-size: 12px;
    color: var(--text-muted);
    line-height: 1.35;
  }

  .strictness-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 10px;
  }

  .field-hint {
    margin: 6px 0 0;
    font-size: 12px;
    color: var(--text-muted);
    line-height: 1.35;
  }

  .action-list {
    display: flex;
    flex-direction: column;
  }

  .action-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 14px 0;
    border-bottom: 1px solid var(--border-light);
  }

  .action-row:last-child {
    border-bottom: none;
  }

  .action-label {
    display: block;
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .action-blurb {
    display: block;
    margin-top: 3px;
    font-size: 12px;
    color: var(--text-muted);
    line-height: 1.35;
  }

  .review-save-bar {
    position: sticky;
    bottom: 0;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 0 4px;
    background: linear-gradient(to top, var(--bg-primary) 70%, transparent);
  }

  .search-wrap {
    position: relative;
  }

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
    border-radius: 8px;
    margin-top: 4px;
    box-shadow: var(--shadow-md);
  }

  .search-item {
    display: block;
    width: 100%;
    text-align: left;
    padding: 10px 12px;
    border: none;
    border-radius: 0;
    background: none;
    font-size: 13px;
    font-family: var(--font-mono);
    color: var(--text-primary);
    cursor: pointer;
  }

  .search-item:hover,
  .search-item.active {
    background: var(--bg-secondary);
    color: var(--text-primary);
    transform: none;
    box-shadow: none;
  }

  .empty-note {
    font-size: 14px;
    color: var(--text-muted);
  }

  .form-row-2 {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 16px 24px;
  }

  .muted {
    font-size: 12px;
    color: var(--text-muted);
  }

  .rules-page-meta {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-top: 16px;
    flex-wrap: wrap;
  }

  textarea {
    width: 100%;
    font-family: inherit;
    font-size: 14px;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg-primary);
    color: var(--text-primary);
    line-height: 1.45;
  }

  @media (max-width: 900px) {
    .settings-shell {
      flex: none;
      min-height: auto;
      overflow: visible;
    }

    .settings-body {
      grid-template-columns: 1fr;
      flex: none;
      min-height: auto;
    }

    .settings-main {
      overflow: visible;
      min-height: auto;
      padding-right: 0;
    }

    .settings-side-nav {
      position: sticky;
      top: 0;
      z-index: 3;
      max-height: none;
      flex-direction: row;
      overflow-x: auto;
      gap: 6px;
      padding: 8px;
      align-self: stretch;
      background: var(--bg-primary);
    }

    .settings-side-item {
      flex: 0 0 auto;
      min-width: 140px;
    }

    .settings-side-item.active {
      box-shadow: inset 0 -2px var(--accent-soft);
    }

    .detector-cards,
    .strictness-grid,
    .form-row-2,
    .settings-meta-grid {
      grid-template-columns: 1fr;
    }
  }
</style>
