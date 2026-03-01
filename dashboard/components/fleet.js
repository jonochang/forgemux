import { html, Card, Dot, RepoPill, MiniBar, SectionLabel, Badge, riskColor, statusColor, contextColor } from "./shared.js";
import { T } from "../theme.js";

const fallbackWorkspace = {
  repos: [],
};

function repoFromPath(path) {
  if (!path) return "unknown";
  const parts = path.split("/").filter(Boolean);
  return parts[parts.length - 1] || path;
}

function statusLabel(state) {
  const lower = (state || "").toLowerCase();
  if (lower === "waitinginput") return "waiting";
  if (lower === "errored") return "errored";
  if (lower === "unreachable") return "unreachable";
  if (lower === "idle") return "idle";
  if (lower === "running") return "running";
  return lower || "unknown";
}

function riskFromState(state) {
  const lower = (state || "").toLowerCase();
  if (lower === "errored") return "red";
  if (lower === "waitinginput") return "yellow";
  if (lower === "unreachable") return "red";
  return "green";
}

export function FleetDashboard({
  sessions,
  workspace = fallbackWorkspace,
  onSelectSession,
  loading = false,
  error = null,
}) {
  const active = sessions.filter((s) => (s.state || "").toLowerCase() === "running").length;
  const blocked = sessions.filter((s) => (s.state || "").toLowerCase() === "waitinginput").length;
  const errored = sessions.filter((s) => (s.state || "").toLowerCase() === "errored").length;

  return html`<div style=${{ padding: "28px" }}>
    <div style=${{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(180px, 1fr))", gap: "16px", marginBottom: "20px" }}>
      <${Card}><${SectionLabel}>Active</${SectionLabel}><div style=${{ fontSize: "28px", marginTop: "8px" }}>${active}</div></${Card}>
      <${Card}><${SectionLabel}>Waiting</${SectionLabel}><div style=${{ fontSize: "28px", marginTop: "8px" }}>${blocked}</div></${Card}>
      <${Card}><${SectionLabel}>Errored</${SectionLabel}><div style=${{ fontSize: "28px", marginTop: "8px" }}>${errored}</div></${Card}>
      <${Card}><${SectionLabel}>Total</${SectionLabel}><div style=${{ fontSize: "28px", marginTop: "8px" }}>${sessions.length}</div></${Card}>
    </div>

    ${loading && html`<div style=${{ color: T.t3, marginTop: "16px" }}>Loading sessions...</div>`}
    ${error && html`<div style=${{ color: T.err, marginTop: "16px" }}>Failed to load sessions.</div>`}
    ${!loading && !error && sessions.length === 0 &&
    html`<div style=${{ color: T.t3, marginTop: "16px" }}>No active sessions.</div>`}

    <div style=${{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(320px, 1fr))", gap: "16px" }}>
      ${sessions.map((session) => {
        const risk = riskFromState(session.state);
        const stateLabel = statusLabel(session.state);
        const repos = [repoFromPath(session.repo_root)];
        const contextPct = session.context_pct || 0;
        return html`<${Card}
          style=${{ border: `1px solid ${riskColor(risk)}`, cursor: onSelectSession ? "pointer" : "default" }}
          onClick=${() => onSelectSession?.(session.id)}
        >
          <div style=${{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
            <div style=${{ display: "flex", alignItems: "center", gap: "10px" }}>
              <${Dot} color=${riskColor(risk)} size=${10} pulse=${risk === "red"} />
              <div>
                <div style=${{ fontWeight: 600, color: T.t1 }}>${session.goal || "(no goal set)"}</div>
                <div style=${{ fontSize: "12px", color: T.t3 }}>${session.id} · ${session.model || "model"}</div>
              </div>
            </div>
            <${Badge} color=${statusColor(stateLabel)} bg=${T.bg3}>${stateLabel}</${Badge}>
          </div>
          <div style=${{ display: "flex", gap: "8px", flexWrap: "wrap", marginTop: "12px" }}>
            ${repos.map((repo) => html`<${RepoPill} repoId=${repo} repos=${workspace.repos} />`)}
          </div>
          <div style=${{ marginTop: "14px" }}>
            <div style=${{ display: "flex", justifyContent: "space-between", fontSize: "12px", color: T.t3 }}>
              <span>Context</span>
              <span>${contextPct}%</span>
            </div>
            <${MiniBar} value=${contextPct} max=${100} color=${contextColor(contextPct)} />
          </div>
        </${Card}>`;
      })}
    </div>
  </div>`;
}
