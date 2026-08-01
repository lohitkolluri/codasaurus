<script>
  import { onMount } from "svelte";
  import { api } from "../../stores/api.js";
  import { currentUser, isOwner, roleLabel } from "../../stores/auth.js";
  import Sidebar from "../../lib/Sidebar.svelte";
  import Header from "../../lib/Header.svelte";
  import LoadingSpinner from "../../lib/LoadingSpinner.svelte";
  import ErrorState from "../../lib/ErrorState.svelte";
  import EmptyState from "../../lib/EmptyState.svelte";
  import UserAvatar from "../../lib/UserAvatar.svelte";

  const PAGE_SIZE = 12;
  const ROLE_FILTERS = [
    { id: "all", label: "All" },
    { id: "owner", label: "Owners" },
    { id: "maintainer", label: "Maintainers" },
    { id: "viewer", label: "Viewers" },
  ];

  let loading = $state(true);
  let error = $state("");
  let members = $state([]);
  let pendingInvites = $state([]);
  let teamMsg = $state("");
  let teamMsgError = $state(false);

  let search = $state("");
  let roleFilter = $state("all");
  let page = $state(1);

  let inviteOpen = $state(false);
  let inviteEmail = $state("");
  let inviteRole = $state("viewer");
  let inviteCreating = $state(false);
  let lastInviteUrl = $state("");

  let confirmRemoveId = $state(null);
  let pwCurrent = $state("");
  let pwNew = $state("");
  let pwMsg = $state("");
  let pwSaving = $state(false);
  let accountOpen = $state(false);

  let filteredMembers = $derived.by(() => {
    const q = search.trim().toLowerCase();
    return members.filter((m) => {
      if (roleFilter !== "all" && m.role !== roleFilter) return false;
      if (!q) return true;
      return (
        (m.email || "").toLowerCase().includes(q) ||
        (m.role || "").toLowerCase().includes(q) ||
        (m.auth_provider || "").toLowerCase().includes(q)
      );
    });
  });

  let totalPages = $derived(Math.max(1, Math.ceil(filteredMembers.length / PAGE_SIZE)));
  let pageSafe = $derived(Math.min(Math.max(1, page), totalPages));
  let pageMembers = $derived.by(() => {
    const start = (pageSafe - 1) * PAGE_SIZE;
    return filteredMembers.slice(start, start + PAGE_SIZE);
  });

  let roleCounts = $derived.by(() => {
    const counts = { all: members.length, owner: 0, maintainer: 0, viewer: 0 };
    for (const m of members) {
      if (counts[m.role] != null) counts[m.role] += 1;
    }
    return counts;
  });

  let prevFilterKey = $state("");
  $effect(() => {
    const key = `${roleFilter}|${search}`;
    if (prevFilterKey && prevFilterKey !== key) {
      page = 1;
    }
    prevFilterKey = key;
  });

  function setMsg(text, isError = false) {
    teamMsg = text;
    teamMsgError = isError;
  }

  async function loadTeam() {
    try {
      error = "";
      const u = await api.get("/api/users");
      members = u.users || [];
      if ($isOwner) {
        try {
          const inv = await api.get("/api/users/invites");
          pendingInvites = inv.invites || [];
        } catch {
          pendingInvites = [];
        }
      } else {
        pendingInvites = [];
      }
    } catch (err) {
      error = err.message || "Failed to load team";
      members = [];
      pendingInvites = [];
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    loadTeam();
  });

  async function createInvite() {
    inviteCreating = true;
    lastInviteUrl = "";
    try {
      const res = await api.post("/api/users/invites", {
        email: inviteEmail.trim() || null,
        role: inviteRole,
      });
      lastInviteUrl = res.url || "";
      setMsg("Invite created — copy and share the link.");
      inviteEmail = "";
      await loadTeam();
    } catch (err) {
      setMsg(err.message || "Invite failed", true);
    } finally {
      inviteCreating = false;
    }
  }

  async function copyInviteUrl() {
    if (!lastInviteUrl) return;
    try {
      await navigator.clipboard.writeText(lastInviteUrl);
      setMsg("Invite link copied");
    } catch {
      setMsg("Copy failed — select the URL manually.", true);
    }
  }

  async function revokeInvite(id) {
    try {
      await api.delete(`/api/users/invites/${id}`);
      setMsg("Invite revoked");
      await loadTeam();
    } catch (err) {
      setMsg(err.message || "Revoke failed", true);
    }
  }

  async function changeMemberRole(id, role) {
    try {
      await api.patch(`/api/users/${id}`, { role });
      setMsg("Role updated");
      await loadTeam();
    } catch (err) {
      setMsg(err.message || "Role change failed", true);
    }
  }

  async function transferBootstrap(id) {
    try {
      await api.post(`/api/users/${id}/transfer-bootstrap`);
      setMsg("Superuser transferred");
      await loadTeam();
    } catch (err) {
      setMsg(err.message || "Transfer failed", true);
    }
  }

  async function removeMember(id) {
    try {
      await api.delete(`/api/users/${id}`);
      confirmRemoveId = null;
      setMsg("Member removed");
      await loadTeam();
    } catch (err) {
      setMsg(err.message || "Remove failed", true);
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
</script>

<div class="app-layout">
  <Sidebar />
  <div class="app-main">
    <Header title="Team" />
    <div class="app-content">
      <div class="page-toolbar">
        <div>
          <p class="eyebrow">Access</p>
          <h1 class="page-title">Team</h1>
          <p class="page-description">
            Owners, maintainers, and viewers. Invite with a shareable link — no email server required.
          </p>
        </div>
        {#if $isOwner}
          <button
            class="primary"
            onclick={() => {
              inviteOpen = !inviteOpen;
            }}
          >
            {inviteOpen ? "Close invite" : "Invite member"}
          </button>
        {/if}
      </div>

      <LoadingSpinner loading={loading} />

      {#if error}
        <ErrorState message={error} />
      {:else if !loading}
        {#if teamMsg}
          <p class="save-msg team-toast" class:error={teamMsgError}>{teamMsg}</p>
        {/if}

        {#if $isOwner && inviteOpen}
          <section class="team-invite-panel">
            <h2 class="team-section-title">Create invite link</h2>
            <p class="page-description" style="margin-bottom: var(--space-4)">
              Anyone with the link can join (optional email lock). Links expire in 7 days.
            </p>
            <div class="team-invite-grid">
              <div class="form-group">
                <label for="invite-email">Email lock (optional)</label>
                <input
                  id="invite-email"
                  type="email"
                  bind:value={inviteEmail}
                  placeholder="optional@company.com"
                />
              </div>
              <div class="form-group">
                <label for="invite-role">Role</label>
                <select id="invite-role" bind:value={inviteRole}>
                  <option value="viewer">Viewer</option>
                  <option value="maintainer">Maintainer</option>
                  <option value="owner">Owner</option>
                </select>
              </div>
            </div>
            <div class="save-row">
              <button class="primary" onclick={createInvite} disabled={inviteCreating}>
                {inviteCreating ? "Creating…" : "Create link"}
              </button>
              {#if lastInviteUrl}
                <button onclick={copyInviteUrl}>Copy link</button>
              {/if}
            </div>
            {#if lastInviteUrl}
              <p class="field-hint team-invite-url">{lastInviteUrl}</p>
            {/if}
          </section>
        {/if}

        <section class="team-roster">
          <div class="team-toolbar">
            <div class="team-filters" role="tablist" aria-label="Filter by role">
              {#each ROLE_FILTERS as f}
                <button
                  type="button"
                  class="team-filter"
                  class:active={roleFilter === f.id}
                  role="tab"
                  aria-selected={roleFilter === f.id}
                  onclick={() => (roleFilter = f.id)}
                >
                  {f.label}
                  <span class="team-filter-count">{roleCounts[f.id] ?? 0}</span>
                </button>
              {/each}
            </div>
            <div class="form-group team-search">
              <label class="sr-only" for="member-search">Search members</label>
              <input
                id="member-search"
                type="search"
                bind:value={search}
                placeholder="Search by email…"
              />
            </div>
          </div>

          {#if pageMembers.length === 0}
            <EmptyState
              message={members.length === 0
                ? "No members yet."
                : "No members match this filter."}
            />
          {:else}
            <div class="team-table-wrap">
              <table class="team-table">
                <thead>
                  <tr>
                    <th scope="col">Member</th>
                    <th scope="col">Role</th>
                    <th scope="col">Auth</th>
                    <th scope="col">Joined</th>
                    {#if $isOwner}
                      <th scope="col" class="team-col-actions">Actions</th>
                    {/if}
                  </tr>
                </thead>
                <tbody>
                  {#each pageMembers as m}
                    <tr>
                      <td>
                        <div class="team-member-cell">
                          <UserAvatar email={m.email} size={32} />
                          <div>
                            <div class="team-email">
                              {m.email}
                              {#if m.id === $currentUser?.id}
                                <span class="team-you">you</span>
                              {/if}
                            </div>
                            {#if m.is_bootstrap}
                              <span class="role-badge bootstrap">Superuser</span>
                            {/if}
                          </div>
                        </div>
                      </td>
                      <td>
                        {#if $isOwner && !m.is_bootstrap}
                          <select
                            class="team-role-select"
                            value={m.role}
                            aria-label={`Role for ${m.email}`}
                            onchange={(e) => changeMemberRole(m.id, e.target.value)}
                          >
                            <option value="owner">Owner</option>
                            <option value="maintainer">Maintainer</option>
                            <option value="viewer">Viewer</option>
                          </select>
                        {:else}
                          <span class="role-badge" class:bootstrap={!!m.is_bootstrap}
                            >{roleLabel(m.role, m.is_bootstrap)}</span
                          >
                        {/if}
                      </td>
                      <td class="muted">{m.auth_provider || "—"}</td>
                      <td class="muted">{m.created_at?.slice?.(0, 10) || "—"}</td>
                      {#if $isOwner}
                        <td class="team-col-actions">
                          {#if m.is_bootstrap}
                            <span class="muted">Protected</span>
                          {:else}
                            <div class="team-row-actions">
                              {#if $currentUser?.is_bootstrap && m.role === "owner"}
                                <button class="linkish" onclick={() => transferBootstrap(m.id)}
                                  >Make superuser</button
                                >
                              {/if}
                              {#if confirmRemoveId === m.id}
                                <button class="danger" onclick={() => removeMember(m.id)}
                                  >Confirm</button
                                >
                                <button onclick={() => (confirmRemoveId = null)}>Cancel</button>
                              {:else}
                                <button class="linkish" onclick={() => (confirmRemoveId = m.id)}
                                  >Remove</button
                                >
                              {/if}
                            </div>
                          {/if}
                        </td>
                      {/if}
                    </tr>
                  {/each}
                </tbody>
              </table>
            </div>

            <div class="team-pagination">
              <span class="muted">
                {filteredMembers.length} member{filteredMembers.length === 1 ? "" : "s"}
                {#if filteredMembers.length > PAGE_SIZE}
                  · page {pageSafe} of {totalPages}
                {/if}
              </span>
              {#if totalPages > 1}
                <div class="team-page-btns">
                  <button
                    disabled={pageSafe <= 1}
                    onclick={() => (page = pageSafe - 1)}>Previous</button
                  >
                  <button
                    disabled={pageSafe >= totalPages}
                    onclick={() => (page = pageSafe + 1)}>Next</button
                  >
                </div>
              {/if}
            </div>
          {/if}
        </section>

        {#if $isOwner && pendingInvites.length > 0}
          <section class="team-pending">
            <h2 class="team-section-title">Pending invites ({pendingInvites.length})</h2>
            <div class="team-table-wrap">
              <table class="team-table">
                <thead>
                  <tr>
                    <th scope="col">Invite</th>
                    <th scope="col">Role</th>
                    <th scope="col">Expires</th>
                    <th scope="col">Created by</th>
                    <th scope="col" class="team-col-actions">Actions</th>
                  </tr>
                </thead>
                <tbody>
                  {#each pendingInvites as inv}
                    <tr>
                      <td><strong>{inv.email || "Open link"}</strong></td>
                      <td><span class="role-badge">{roleLabel(inv.role)}</span></td>
                      <td class="muted">{inv.expires_at?.slice?.(0, 10) || "—"}</td>
                      <td class="muted">{inv.created_by || "—"}</td>
                      <td class="team-col-actions">
                        <button class="linkish" onclick={() => revokeInvite(inv.id)}>Revoke</button>
                      </td>
                    </tr>
                  {/each}
                </tbody>
              </table>
            </div>
          </section>
        {/if}

        <section class="team-account">
          <button
            type="button"
            class="team-account-toggle"
            aria-expanded={accountOpen}
            onclick={() => (accountOpen = !accountOpen)}
          >
            <span>
              Your account · {roleLabel($currentUser?.role, $currentUser?.is_bootstrap)} · {$currentUser?.email}
            </span>
            <span class="muted">{accountOpen ? "Hide" : "Show"}</span>
          </button>
          {#if accountOpen}
            <div class="team-account-body">
              {#if $currentUser?.auth_provider !== "oidc"}
                <h3 class="team-section-title">Change password</h3>
                <div class="team-invite-grid">
                  <div class="form-group">
                    <label for="pw-current">Current password</label>
                    <input
                      id="pw-current"
                      type="password"
                      bind:value={pwCurrent}
                      autocomplete="current-password"
                    />
                  </div>
                  <div class="form-group">
                    <label for="pw-new">New password</label>
                    <input
                      id="pw-new"
                      type="password"
                      bind:value={pwNew}
                      placeholder="At least 8 characters"
                      autocomplete="new-password"
                    />
                  </div>
                </div>
                <div class="save-row">
                  <button
                    onclick={changePassword}
                    disabled={pwSaving || !pwCurrent || pwNew.length < 8}
                  >
                    {pwSaving ? "Updating…" : "Update password"}
                  </button>
                  {#if pwMsg}
                    <span class="save-msg" class:error={pwMsg !== "Password updated"}>{pwMsg}</span>
                  {/if}
                </div>
              {:else}
                <p class="muted">Signed in with SSO — password is managed by your identity provider.</p>
              {/if}
            </div>
          {/if}
        </section>
      {/if}
    </div>
  </div>
</div>

<style>
  .team-toast {
    margin-bottom: var(--space-4);
  }

  .team-invite-panel,
  .team-roster,
  .team-pending,
  .team-account {
    margin-bottom: var(--space-8);
    padding-bottom: var(--space-6);
    border-bottom: 1px solid var(--border-light);
  }

  .team-account {
    border-bottom: none;
  }

  .team-section-title {
    margin: 0 0 var(--space-2);
    font-size: var(--text-lg);
    font-weight: var(--weight-semibold);
    letter-spacing: -0.02em;
  }

  .team-invite-grid {
    display: grid;
    grid-template-columns: 1fr 180px;
    gap: var(--space-4);
  }

  @media (max-width: 640px) {
    .team-invite-grid {
      grid-template-columns: 1fr;
    }
  }

  .team-invite-url {
    margin-top: var(--space-3);
    word-break: break-all;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
  }

  .team-toolbar {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-4);
    align-items: center;
    justify-content: space-between;
    margin-bottom: var(--space-4);
  }

  .team-filters {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .team-filter {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 6px 10px;
    font-size: var(--text-sm);
    background: transparent;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
  }

  .team-filter.active {
    border-color: var(--text-primary);
    color: var(--text-primary);
    background: color-mix(in srgb, var(--text-primary) 6%, transparent);
  }

  .team-filter-count {
    font-variant-numeric: tabular-nums;
    font-size: var(--text-xs);
    color: var(--text-muted);
  }

  .team-search {
    margin: 0;
    min-width: min(240px, 100%);
  }

  .team-search input {
    width: 100%;
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

  .team-table-wrap {
    overflow-x: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
  }

  .team-table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }

  .team-table th,
  .team-table td {
    text-align: left;
    padding: 12px 14px;
    border-bottom: 1px solid var(--border-light);
    vertical-align: middle;
  }

  .team-table th {
    font-size: var(--text-xs);
    font-weight: var(--weight-medium);
    color: var(--text-muted);
    background: var(--bg-secondary);
  }

  .team-table tbody tr:last-child td {
    border-bottom: none;
  }

  .team-member-cell {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .team-email {
    font-weight: var(--weight-medium);
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
  }

  .team-you {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-muted);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 1px 5px;
  }

  .team-role-select {
    font-size: var(--text-sm);
    max-width: 140px;
  }

  .team-col-actions {
    text-align: right;
    white-space: nowrap;
  }

  .team-row-actions {
    display: inline-flex;
    flex-wrap: wrap;
    gap: 8px;
    justify-content: flex-end;
  }

  .team-pagination {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    margin-top: var(--space-4);
    flex-wrap: wrap;
  }

  .team-page-btns {
    display: flex;
    gap: 8px;
  }

  .team-account-toggle {
    width: 100%;
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--space-4);
    padding: 12px 0;
    background: transparent;
    border: none;
    border-radius: 0;
    text-align: left;
    font-size: var(--text-sm);
    color: var(--text-primary);
  }

  .team-account-toggle:hover {
    background: transparent;
  }

  .team-account-body {
    padding-top: var(--space-2);
  }

  .muted {
    color: var(--text-muted);
    font-size: var(--text-sm);
  }
</style>
