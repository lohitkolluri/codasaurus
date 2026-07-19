<script>
  import { onMount } from "svelte";
  import { push } from "svelte-spa-router";
  import { api } from "../../stores/api.js";

  let status = $state(null);
  let loading = $state(true);

  onMount(async () => {
    try {
      status = await api.get("/api/setup/status");
      if (!status.complete) {
        if (!status.database) push("/setup/database");
        else if (!status.llm) push("/setup/llm");
        else if (!status.github) push("/setup/github");
        else if (!status.admin) push("/setup/admin");
      }
    } catch {
      // ignore — render the page anyway
    } finally {
      loading = false;
    }
    startConfetti();
  });

  function startConfetti() {
    const canvas = document.createElement("canvas");
    canvas.style.cssText = "position:fixed;top:0;left:0;width:100%;height:100%;pointer-events:none;z-index:9999";
    canvas.width = window.innerWidth;
    canvas.height = window.innerHeight;
    document.body.appendChild(canvas);

    const ctx = canvas.getContext("2d");
    const colors = ["#f44336","#e91e63","#9c27b0","#673ab7","#3f51b5","#2196f3","#009688","#4caf50","#ff9800","#ff5722"];
    const pieces = Array.from({ length: 150 }, () => ({
      x: Math.random() * canvas.width,
      y: Math.random() * canvas.height - canvas.height,
      w: Math.random() * 10 + 5,
      h: Math.random() * 6 + 3,
      color: colors[Math.floor(Math.random() * colors.length)],
      rot: Math.random() * 360,
      rv: (Math.random() - 0.5) * 6,
      vx: (Math.random() - 0.5) * 3,
      vy: Math.random() * 3 + 2,
    }));

    let start = Date.now();
    let frame;

    function draw() {
      const elapsed = Date.now() - start;
      if (elapsed > 6000) {
        canvas.remove();
        return;
      }

      ctx.clearRect(0, 0, canvas.width, canvas.height);

      // Fade out after 4s
      const opacity = Math.min(1, (6000 - elapsed) / 2000);
      ctx.globalAlpha = opacity;

      for (const p of pieces) {
        p.x += p.vx;
        p.y += p.vy;
        p.vy += 0.05;
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

    draw();
  }
</script>

<div class="wizard-card" style="text-align:center;padding-top:80px">
  {#if loading}
    <p style="color:var(--text-muted)">Verifying setup…</p>
  {:else}
    <div class="step-indicator">
      <span class="step-dot completed"></span>
      <span class="step-dot completed"></span>
      <span class="step-dot completed"></span>
      <span class="step-dot completed"></span>
    </div>

    <h2 style="font-size:24px;font-weight:700;margin-bottom:12px">Setup Complete</h2>
    <p style="color:var(--text-muted);margin-bottom:40px;max-width:400px;margin-left:auto;margin-right:auto">
      Codasaurus is configured and ready. Your database, LLM, GitHub integration, and admin account have been set up.
    </p>

    <div style="text-align:left;margin-bottom:40px;max-width:400px;margin-left:auto;margin-right:auto">
      <div style="padding:8px 0;border-bottom:1px solid var(--border);display:flex;justify-content:space-between">
        <span style="color:var(--text-muted)">Database</span>
        <span style="font-weight:500;color:var(--success)">Configured</span>
      </div>
      <div style="padding:8px 0;border-bottom:1px solid var(--border);display:flex;justify-content:space-between">
        <span style="color:var(--text-muted)">LLM</span>
        <span style="font-weight:500;color:var(--success)">Configured</span>
      </div>
      <div style="padding:8px 0;border-bottom:1px solid var(--border);display:flex;justify-content:space-between">
        <span style="color:var(--text-muted)">GitHub</span>
        <span style="font-weight:500;color:var(--success)">Configured</span>
      </div>
      <div style="padding:8px 0;display:flex;justify-content:space-between">
        <span style="color:var(--text-muted)">Admin</span>
        <span style="font-weight:500;color:var(--success)">Created</span>
      </div>
    </div>

    <button class="primary" style="font-size:16px;padding:12px 40px" onclick={() => push("/login")}>
      Go to Dashboard
    </button>
  {/if}
</div>
