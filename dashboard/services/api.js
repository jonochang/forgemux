const tokenKey = "forgemux_token";

function authHeaders() {
  const token = localStorage.getItem(tokenKey);
  return token ? { Authorization: `Bearer ${token}` } : {};
}

export async function fetchJSON(path, opts = {}) {
  const headers = { "content-type": "application/json", ...authHeaders(), ...(opts.headers || {}) };
  const res = await fetch(path, { ...opts, headers });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(text || res.statusText);
  }
  return res.json();
}

export const api = {
  sessions() {
    return fetchJSON("/sessions");
  },
  decisions(workspaceId) {
    const qs = new URLSearchParams({ workspace_id: workspaceId });
    return fetchJSON(`/decisions?${qs.toString()}`);
  },
};
