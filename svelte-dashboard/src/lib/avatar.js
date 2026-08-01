/**
 * Automatic profile pictures via DiceBear's free HTTP API.
 * https://www.dicebear.com/ — deterministic SVG from a seed.
 *
 * Major free CDNs don't ship a literal T-rex style; `bottts` is the classic
 * funky creature look (fits Codasaurus). Email is hashed before leaving the
 * browser so the raw address isn't sent as the seed.
 */

function fnv1a(str) {
  let h = 0x811c9dc5;
  for (let i = 0; i < str.length; i++) {
    h ^= str.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
}

/** Stable seed from email (privacy: don't send raw email to the CDN). */
export function avatarSeed(email) {
  const normalized = (email || "guest").trim().toLowerCase();
  return `codasaurus-${fnv1a(normalized).toString(16)}`;
}

export function avatarInitials(email) {
  const local = (email || "?").split("@")[0] || "?";
  const parts = local.split(/[._\-+]+/).filter(Boolean);
  if (parts.length >= 2) {
    return (parts[0][0] + parts[1][0]).toUpperCase();
  }
  return local.slice(0, 2).toUpperCase();
}

/**
 * DiceBear bottts URL — funky creature avatar, same seed → same art.
 * @see https://www.dicebear.com/styles/bottts
 */
export function avatarUrl(email, size = 64) {
  const params = new URLSearchParams({
    seed: avatarSeed(email),
    size: String(Math.max(48, size)),
    radius: "18",
    backgroundType: "gradientLinear",
    // Punchy backgrounds so pfps pop in the roster
    backgroundColor: "b6e3f4,c0aede,d1d4f9,ffd5dc,ffdfbf,c1f0c1",
  });
  return `https://api.dicebear.com/9.x/bottts/svg?${params.toString()}`;
}

/** @deprecated use avatarUrl — kept for any leftover imports */
export function identiconSvg(email, size = 64) {
  return avatarUrl(email, size);
}
