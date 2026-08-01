/** Shared first-time setup step definitions and helpers. */

export const SETUP_STEPS = [
  {
    key: "database",
    label: "Database",
    short: "DB",
    desc: "Where Codasaurus stores reviews and settings",
    route: "/setup/database",
    eta: "~30s",
  },
  {
    key: "llm",
    label: "AI review",
    short: "AI",
    desc: "Optional BYOK LLM — Tier-1 detectors work without it",
    route: "/setup/llm",
    eta: "~1 min",
    optional: true,
  },
  {
    key: "github",
    label: "GitHub App",
    short: "GH",
    desc: "Connect repos so Codasaurus can review PRs",
    route: "/setup/github",
    eta: "~2 min",
  },
  {
    key: "admin",
    label: "Admin",
    short: "You",
    desc: "Create the account you'll use to sign in",
    route: "/setup/admin",
    eta: "~30s",
  },
];

export function stepIndex(key) {
  return SETUP_STEPS.findIndex((s) => s.key === key);
}

export function firstIncomplete(status) {
  if (!status) return SETUP_STEPS[0];
  return SETUP_STEPS.find((s) => !status[s.key]) ?? null;
}

export function completedCount(status) {
  if (!status) return 0;
  return SETUP_STEPS.filter((s) => status[s.key]).length;
}

export function isTruthy(v) {
  return ["true", "1", "yes", "on"].includes(String(v ?? "").toLowerCase());
}
