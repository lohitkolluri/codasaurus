import { writable } from "svelte/store";
import { api } from "./api.js";

export const currentUser = writable(null);
export const authLoading = writable(true);

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
