import { derived, writable } from "svelte/store";
import { api } from "./api.js";

export const currentUser = writable(null);
export const authLoading = writable(true);

export const isOwner = derived(
  currentUser,
  ($u) => $u?.role === "owner" || $u?.role === "admin",
);
export const isMaintainer = derived(
  currentUser,
  ($u) =>
    $u?.role === "owner" || $u?.role === "admin" || $u?.role === "maintainer",
);
export const isViewer = derived(currentUser, ($u) => $u?.role === "viewer");

export function roleLabel(role, isBootstrap = false) {
  if (isBootstrap) return "Superuser";
  if (role === "admin") return "Owner";
  if (!role) return "";
  return role.charAt(0).toUpperCase() + role.slice(1);
}

export async function checkSession() {
  authLoading.set(true);
  try {
    const res = await api.get("/api/auth/me");
    currentUser.set(res.user ?? null);
  } catch {
    currentUser.set(null);
  } finally {
    authLoading.set(false);
  }
}

export async function login(email, password) {
  const res = await api.post("/api/auth/login", { email, password });
  currentUser.set(res.user ?? null);
  return res.user;
}

export async function logout() {
  try {
    await api.post("/api/auth/logout");
  } catch {
    // ignore
  }
  currentUser.set(null);
}
