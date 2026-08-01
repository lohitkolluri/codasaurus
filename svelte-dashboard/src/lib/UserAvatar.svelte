<script>
  import { avatarUrl, avatarInitials } from "./avatar.js";

  let {
    email = "",
    size = 32,
    class: className = "",
  } = $props();

  let failed = $state(false);
  let src = $derived(avatarUrl(email || "guest", Math.max(size * 2, 64)));
  let initials = $derived(avatarInitials(email));
  let label = $derived(email ? `Avatar for ${email}` : "User avatar");

  // Reset fallback when email/src changes
  $effect(() => {
    src;
    failed = false;
  });
</script>

{#if failed}
  <span
    class="user-avatar user-avatar-initials {className}"
    style={`width:${size}px;height:${size}px;font-size:${Math.max(10, Math.round(size * 0.34))}px`}
    aria-hidden="true"
    title={label}
  >
    {initials}
  </span>
{:else}
  <img
    class="user-avatar {className}"
    src={src}
    width={size}
    height={size}
    alt=""
    aria-label={label}
    draggable="false"
    loading="lazy"
    decoding="async"
    onerror={() => (failed = true)}
  />
{/if}

<style>
  .user-avatar {
    display: inline-block;
    border-radius: 8px;
    flex-shrink: 0;
    object-fit: cover;
    vertical-align: middle;
    background: var(--bg-secondary);
  }

  .user-avatar-initials {
    display: inline-grid;
    place-items: center;
    font-weight: 600;
    letter-spacing: 0.02em;
    background: color-mix(in srgb, var(--accent-soft) 14%, var(--bg-secondary));
    color: var(--text-primary);
  }
</style>
