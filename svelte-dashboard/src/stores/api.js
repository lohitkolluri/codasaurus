const BASE_URL = "";
const REQUEST_TIMEOUT_MS = 30000;

async function request(method, path, body) {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), REQUEST_TIMEOUT_MS);

  try {
    const opts = {
      method,
      headers: { "Content-Type": "application/json" },
      credentials: "same-origin",
      signal: controller.signal,
    };

    if (body !== undefined) {
      opts.body = JSON.stringify(body);
    }

    const res = await fetch(`${BASE_URL}${path}`, opts);

    if (res.status === 204) {
      return null;
    }

    // Try to parse JSON even for error responses
    let data;
    try {
      data = await res.json();
    } catch {
      if (!res.ok) {
        throw new Error(`Request failed: ${res.status} ${res.statusText}`);
      }
      return null;
    }

    if (!res.ok) {
      const err = new Error(data.error || data.message || `Request failed: ${res.status}`);
      err.status = res.status;
      throw err;
    }

    return data;
  } finally {
    clearTimeout(timeout);
  }
}

export const api = {
  get: (path) => request("GET", path),
  post: (path, body) => request("POST", path, body),
  put: (path, body) => request("PUT", path, body),
  delete: (path) => request("DELETE", path),
};
