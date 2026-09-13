<script>
  import { api } from "../stores/api.js";

  let { canEdit = false } = $props();

  let servers = $state([]);
  let loading = $state(true);
  let msg = $state("");
  let savingId = $state("");
  let testingId = $state("");

  // Per-server draft fields (api key / base url / name), keyed by server id.
  let drafts = $state({});

  let showAddCustom = $state(false);
  let newId = $state("");
  let newName = $state("");
  let newBaseUrl = $state("");
  let newApiKey = $state("");

  async function load() {
    loading = true;
    try {
      const res = await api.get("/api/mcp/servers");
      servers = res?.servers || [];
      for (const s of servers) {
        drafts[s.id] ??= { apiKey: "", baseUrl: s.base_url, name: s.name };
      }
    } catch (err) {
      msg = err.message || "Failed to load MCP connectors";
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    load();
  });

  async function saveServer(server) {
    savingId = server.id;
    msg = "";
    try {
      const draft = drafts[server.id] || {};
      const body = { enabled: server.enabled };
      if (draft.apiKey) body.api_key = draft.apiKey;
      if (server.is_custom) {
        body.base_url = draft.baseUrl;
        body.name = draft.name;
      }
      await api.put(`/api/mcp/servers/${server.id}`, body);
      drafts[server.id].apiKey = "";
      msg = `Saved ${server.name}`;
      await load();
    } catch (err) {
      msg = err.message || "Save failed";
    } finally {
      savingId = "";
    }
  }

  async function testServer(server) {
    testingId = server.id;
    msg = "";
    try {
      const res = await api.post(`/api/mcp/${server.id}/test`, {});
      msg = `${server.name}: OK (${res?.tool_count ?? 0} tools)`;
    } catch (err) {
      msg = err.message || `${server.name} test failed`;
    } finally {
      testingId = "";
    }
  }

  async function removeServer(server) {
    savingId = server.id;
    msg = "";
    try {
      await api.delete(`/api/mcp/servers/${server.id}`);
      msg = `Removed ${server.name}`;
      await load();
    } catch (err) {
      msg = err.message || "Remove failed";
    } finally {
      savingId = "";
    }
  }

  async function addCustomServer() {
    const id = newId.trim().toLowerCase().replace(/[^a-z0-9_-]/g, "-");
    if (!id || !newBaseUrl.trim()) {
      msg = "Server id and base URL are required";
      return;
    }
    savingId = id;
    msg = "";
    try {
      await api.put(`/api/mcp/servers/${id}`, {
        enabled: true,
        base_url: newBaseUrl.trim(),
        name: newName.trim() || id,
        api_key: newApiKey || undefined,
      });
      newId = "";
      newName = "";
      newBaseUrl = "";
      newApiKey = "";
      showAddCustom = false;
      msg = "Custom server added";
      await load();
    } catch (err) {
      msg = err.message || "Add failed";
    } finally {
      savingId = "";
    }
  }
</script>

<section class="card settings-card">
  <header class="settings-section-head">
    <h3 class="section-heading">MCP Connectors</h3>
    <p class="section-desc">
      Let the review LLM call external tools during PR review (e.g. Context7 for current library docs).
      Enable per-repo with <code>mcp_tools = true</code> under <code>[checks]</code> in <code>.codasaurus.toml</code> — off everywhere until a repo opts in.
    </p>
  </header>

  {#if loading}
    <p class="empty-note">Loading…</p>
  {:else}
    {#each servers as server (server.id)}
      <div class="detector-row" style="flex-direction:column;align-items:stretch;gap:8px;padding:12px 0">
        <div style="display:flex;align-items:center;justify-content:space-between;gap:8px">
          <div>
            <strong>{server.name}</strong>
            {#if server.docs_url}
              <a href={server.docs_url} target="_blank" rel="noopener noreferrer" style="margin-left:6px;font-size:12px">docs</a>
            {/if}
            <div class="field-hint">{server.base_url}{server.key_configured ? " · key configured" : ""}</div>
          </div>
          <label class="toggle">
            <div class="toggle-track" class:on={server.enabled} role="checkbox" aria-checked={server.enabled}
              tabindex="0"
              onclick={() => canEdit && (server.enabled = !server.enabled)}
              onkeydown={(e) => { if (canEdit && (e.key === 'Enter' || e.key === ' ')) { e.preventDefault(); server.enabled = !server.enabled; } }}>
              <div class="toggle-knob"></div>
            </div>
          </label>
        </div>

        {#if server.is_custom}
          <div class="form-row-2">
            <div class="form-group">
              <label for={`mcp-name-${server.id}`}>Name</label>
              <input id={`mcp-name-${server.id}`} type="text" bind:value={drafts[server.id].name} disabled={!canEdit} />
            </div>
            <div class="form-group">
              <label for={`mcp-url-${server.id}`}>Base URL</label>
              <input id={`mcp-url-${server.id}`} type="url" bind:value={drafts[server.id].baseUrl} disabled={!canEdit} />
            </div>
          </div>
        {/if}
        <div class="form-group">
          <label for={`mcp-key-${server.id}`}>API key</label>
          <input id={`mcp-key-${server.id}`} type="password" autocomplete="off"
            placeholder={server.key_configured ? "•••••••• (unchanged)" : "Optional"}
            bind:value={drafts[server.id].apiKey} disabled={!canEdit} />
        </div>

        <div class="save-row">
          <button onclick={() => saveServer(server)} disabled={!canEdit || savingId === server.id}>
            {savingId === server.id ? "Saving…" : "Save"}
          </button>
          <button type="button" onclick={() => testServer(server)} disabled={!canEdit || testingId === server.id}>
            {testingId === server.id ? "Testing…" : "Test"}
          </button>
          {#if server.is_custom}
            <button type="button" class="danger" onclick={() => removeServer(server)} disabled={!canEdit || savingId === server.id}>Remove</button>
          {/if}
        </div>
      </div>
    {/each}

    {#if canEdit}
      {#if !showAddCustom}
        <div class="save-row" style="margin-top:8px">
          <button type="button" onclick={() => (showAddCustom = true)}>Add custom server</button>
        </div>
      {:else}
        <div class="detector-row" style="flex-direction:column;align-items:stretch;gap:8px;padding:12px 0">
          <strong>Custom server</strong>
          <div class="form-row-2">
            <div class="form-group">
              <label for="mcp-new-id">Server id</label>
              <input id="mcp-new-id" type="text" bind:value={newId} placeholder="my-server" />
            </div>
            <div class="form-group">
              <label for="mcp-new-name">Name</label>
              <input id="mcp-new-name" type="text" bind:value={newName} placeholder="My Server" />
            </div>
          </div>
          <div class="form-group">
            <label for="mcp-new-url">Base URL</label>
            <input id="mcp-new-url" type="url" bind:value={newBaseUrl} placeholder="https://mcp.example.com/mcp" />
          </div>
          <div class="form-group">
            <label for="mcp-new-key">API key</label>
            <input id="mcp-new-key" type="password" autocomplete="off" bind:value={newApiKey} />
          </div>
          <div class="save-row">
            <button onclick={addCustomServer} disabled={savingId === newId.trim().toLowerCase()}>Add</button>
            <button type="button" onclick={() => (showAddCustom = false)}>Cancel</button>
          </div>
        </div>
      {/if}
    {/if}

    {#if msg}<p class="save-msg" class:error={/fail|error|reject|missing|invalid|required/i.test(msg)}>{msg}</p>{/if}
  {/if}
</section>
