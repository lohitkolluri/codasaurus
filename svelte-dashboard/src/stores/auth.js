import { writable } from "svelte/store";
import { api } from "./api.js";

export const currentUser = writable(null);
export const authLoading = writable(true);

export async function checkSession() {
  authLoading.set(true);
  try {
    const user = await api.get("/api/auth/me");
    currentUser.set(user);
  } catch {
    currentUser.set(null);
  } finally {
    authLoading.set(false);
  }
}

export async function login(email, password) {
  const user = await api.post("/api/auth/login", { email, password });
  currentUser.set(user);
  return user;
}

export async function logout() {
  try {
    await api.post("/api/auth/logout");
  } catch {
    // ignore
  }
  currentUser.set(null);
}
