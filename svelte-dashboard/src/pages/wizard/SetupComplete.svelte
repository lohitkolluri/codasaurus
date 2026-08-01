<script>
  import { onMount, onDestroy } from "svelte";
  import { push } from "svelte-spa-router";
  import { api } from "../../stores/api.js";
  import WizardShell from "../../lib/WizardShell.svelte";

  let status = $state(null);
  let loading = $state(true);
  let installUrl = $state("");
  let frame = null;
  let canvasEl = null;

  onMount(async () => {
    try {
      status = await api.get("/api/setup/status");
      if (!status?.complete) {
        if (!status?.database) push("/setup/database");
        else if (!status?.llm) push("/setup/llm");
        else if (!status?.github) push("/setup/github");
        else if (!status?.admin) push("/setup/admin");
        else push("/setup");
        return;
      }
      installUrl =
        status.github_install_url ||
        "https://github.com/settings/installations";
      loading = false;
      if (typeof window !== "undefined" && !window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
        startConfetti();
      }
    } catch {
      push("/setup");
    }
  });

  function startConfetti() {
    canvasEl = document.createElement("canvas");
    canvasEl.style.cssText =
      "position:fixed;top:0;left:0;width:100%;height:100%;pointer-events:none;z-index:9999";
    canvasEl.width = window.innerWidth;
    canvasEl.height = window.innerHeight;
    document.body.appendChild(canvasEl);

    const ctx = canvasEl.getContext("2d");
    const colors = ["#ff6659", "#f4f4f5", "#a1a1aa", "#22c55e", "#3b82f6"];
    const pieces = Array.from({ length: 80 }, () => ({
      x: Math.random() * canvasEl.width,
      y: Math.random() * canvasEl.height - canvasEl.height,
      w: Math.random() * 8 + 4,
      h: Math.random() * 5 + 2,
      color: colors[Math.floor(Math.random() * colors.length)],
      rot: Math.random() * 360,
      rv: (Math.random() - 0.5) * 5,
      vx: (Math.random() - 0.5) * 2.5,
      vy: Math.random() * 2.5 + 1.5,
    }));

    const start = Date.now();
    function draw() {
      const elapsed = Date.now() - start;
      if (elapsed > 4500) {
        canvasEl?.remove();
        return;
      }
      ctx.clearRect(0, 0, canvasEl.width, canvasEl.height);
      ctx.globalAlpha = Math.min(1, (4500 - elapsed) / 1200);
      for (const p of pieces) {
        p.x += p.vx;
        p.y += p.vy;
        p.vy += 0.04;
        p.rot += p.rv;
        ctx.save();
        ctx.translate(p.x, p.y);
        ctx.rotate((p.rot * Math.PI) / 180);
        ctx.fillStyle = p.color;
        ctx.fillRect(-p.w / 2, -p.h / 2, p.w, p.h);
        ctx.restore();
      }
      frame = requestAnimationFrame(draw);
    }
    frame = requestAnimationFrame(draw);
  }

  onDestroy(() => {
    if (frame) cancelAnimationFrame(frame);
    canvasEl?.remove();
  });
</script>

{#if loading}
  <WizardShell showProgress={false}>
    <p style="color:var(--text-muted);text-align:center;margin:48px 0">Verifying setup…</p>
  </WizardShell>
{:else}
  <WizardShell
    showProgress={false}
    title="You're ready to review PRs"
    subtitle="Codasaurus is configured. One more step: install the App, then open a pull request."
  >
    <ol class="wiz-activation">
      <li>
        <span class="wiz-activation-num">1</span>
        <div>
          <strong>Install the GitHub App</strong>
          <p>Pick the org/user and repositories Codasaurus should watch.</p>
        </div>
      </li>
      <li>
        <span class="wiz-activation-num">2</span>
        <div>
          <strong>Open or update a PR</strong>
          <p>Codasaurus posts a walkthrough + Tier-1 findings automatically.</p>
        </div>
      </li>
      <li>
        <span class="wiz-activation-num">3</span>
        <div>
          <strong>Mention @codasaurus</strong>
          <p>Try <code>review</code>, <code>describe</code>, or <code>help</code> on a PR comment.</p>
        </div>
      </li>
    </ol>

    <div class="wizard-actions" style="border:none;padding-top:0;flex-direction:column;gap:10px">
      {#if installUrl}
        <button class="primary" style="width:100%;padding:12px" onclick={() => window.open(installUrl, "_blank", "noopener,noreferrer")}>
          Install on GitHub
        </button>
      {/if}
      <button class="primary" style="width:100%;padding:12px" onclick={() => push("/login")}>
        Sign in to dashboard
      </button>
    </div>

    <p class="wizard-hint" style="text-align:center;margin-top:16px">
      Need the docs? See <code>docs/setup-onboarding.md</code> and <code>docs/setup-github-app.md</code>.
    </p>
  </WizardShell>
{/if}
