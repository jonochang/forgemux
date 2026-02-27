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
  approveDecision(id, body) {
    return fetchJSON(`/decisions/${id}/approve`, {
      method: "POST",
      body: JSON.stringify(body),
    });
  },
  denyDecision(id, body) {
    return fetchJSON(`/decisions/${id}/deny`, {
      method: "POST",
      body: JSON.stringify(body),
    });
  },
  commentDecision(id, body) {
    return fetchJSON(`/decisions/${id}/comment`, {
      method: "POST",
      body: JSON.stringify(body),
    });
  },
  replayTimeline(sessionId, after, limit) {
    const qs = new URLSearchParams();
    if (after != null) qs.set("after", after);
    if (limit != null) qs.set("limit", limit);
    const suffix = qs.toString();
    return fetchJSON(`/sessions/${sessionId}/replay/timeline${suffix ? `?${suffix}` : ""}`);
  },
  replayDiff(sessionId) {
    return fetchJSON(`/sessions/${sessionId}/replay/diff`);
  },
  replayTerminal(sessionId) {
    return fetchJSON(`/sessions/${sessionId}/replay/terminal`);
  },
};
