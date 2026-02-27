import { useMemo } from "../lib/hooks.module.js";
import { html, Badge, Dot, RepoPill, SectionLabel } from "./shared.js";
import { T } from "../theme.js";

function eventColor(type) {
  switch (type) {
    case "decision":
      return T.err;
    case "test":
      return T.ok;
    case "switch":
      return T.info;
    case "edit":
      return T.ember;
    case "tool":
      return T.purple;
    case "read":
      return T.t2;
    default:
      return T.t4;
  }
}

function formatTime(evt) {
  if (evt.elapsed) return evt.elapsed;
  if (evt.timestamp) return new Date(evt.timestamp).toLocaleTimeString();
  return "";
}

export function SessionReplay({ session, workspace, events, tab, onTabChange, diff, terminal }) {
  const repoMap = useMemo(() => {
    const map = new Map();
    (workspace.repos || []).forEach((repo) => map.set(repo.id, repo));
    return map;
  }, [workspace.repos]);

  if (!session) {
    return html`<div style=${{ padding: "28px", color: T.t3 }}>No session selected.</div>`;
  }

  return html`<div style=${{ display: "flex", height: "calc(100vh - 72px)", overflow: "hidden" }}>
    <div style=${{ width: "320px", borderRight: `1px solid ${T.border}`, background: T.bg1, display: "flex", flexDirection: "column" }}>
      <div style=${{ padding: "16px", borderBottom: `1px solid ${T.border}` }}>
        <div style=${{ display: "flex", alignItems: "center", gap: "8px", marginBottom: "8px" }}>
          <${Dot} color=${eventColor("system")} size=${8} />
          <span style=${{ fontFamily: T.mono, fontSize: "12px", color: T.t3 }}>${session.id}</span>
          <${Badge} color=${T.info} bg=${T.infoS}>${session.model}</${Badge}>
        </div>
        <div style=${{ fontSize: "14px", fontWeight: 600, color: T.t1, marginBottom: "8px" }}>
          ${session.goal || "No goal recorded"}
        </div>
        <div style=${{ display: "flex", gap: "6px", flexWrap: "wrap" }}>
          ${(session.repos || []).map((repoId) => html`<${RepoPill} repoId=${repoId} repos=${workspace.repos || []} />`)}
        </div>
      </div>

      <div style=${{ flex: 1, overflowY: "auto", padding: "12px 0" }}>
        <div style=${{ padding: "0 16px 8px" }}><${SectionLabel}>Timeline</${SectionLabel}></div>
        ${(events || []).map((evt, idx) => {
          const repo = evt.repo_id ? repoMap.get(evt.repo_id) : null;
          const color = eventColor(evt.event_type);
          return html`<div key=${evt.id || idx} style=${{ display: "flex", gap: "10px", padding: "6px 16px" }}>
            <div style=${{ display: "flex", flexDirection: "column", alignItems: "center", width: "18px" }}>
              <${Dot} color=${color} size=${6} />
              ${idx < events.length - 1 &&
              html`<div style=${{ width: "1px", flex: 1, minHeight: "12px", background: `${color}55` }}></div>`}
            </div>
            <div style=${{ flex: 1 }}>
              <div style=${{ display: "flex", alignItems: "center", gap: "6px" }}>
                <span style=${{ fontSize: "10px", fontFamily: T.mono, color: T.t4 }}>${formatTime(evt)}</span>
                ${repo &&
                html`<span style=${{ fontSize: "10px", fontFamily: T.mono, color: repo.color }}>
                  ${repo.icon} ${repo.label}
                </span>`}
              </div>
              <div style=${{ fontSize: "12px", color: T.t2, marginTop: "2px", lineHeight: 1.4 }}>
                ${evt.action}
              </div>
            </div>
          </div>`;
        })}
      </div>
    </div>

    <div style=${{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden" }}>
      <div style=${{ display: "flex", gap: "6px", borderBottom: `1px solid ${T.border}`, padding: "8px 16px", background: T.bg2 }}>
        ${[
          { key: "diff", label: "Unified Diff" },
          { key: "log", label: "Structured Log" },
          { key: "terminal", label: "Terminal" },
        ].map(
          (tabInfo) => html`<button
            key=${tabInfo.key}
            onClick=${() => onTabChange(tabInfo.key)}
            style=${{
              background: "transparent",
              border: "none",
              cursor: "pointer",
              padding: "8px 10px",
              fontSize: "12px",
              color: tab === tabInfo.key ? T.t1 : T.t3,
              borderBottom: tab === tabInfo.key ? `2px solid ${T.ember}` : "2px solid transparent",
            }}
          >${tabInfo.label}</button>`
        )}
      </div>

      <div style=${{ flex: 1, overflowY: "auto", padding: "20px" }}>
        ${tab === "diff" &&
        html`<div>
          ${(diff?.groups || []).length === 0 &&
          html`<div style=${{ color: T.t3 }}>No diffs captured yet.</div>`}
          ${(diff?.groups || []).map(
            (group) => html`<div style=${{ marginBottom: "18px" }}>
              <div style=${{ fontWeight: 600, marginBottom: "6px" }}>${group.repo}</div>
              ${(group.files || []).map(
                (file) => html`<div style=${{ fontFamily: T.mono, fontSize: "12px", color: T.t2 }}>
                  ${file.path} (+${file.additions} / -${file.deletions})
                </div>`
              )}
            </div>`
          )}
        </div>`}
        ${tab === "log" &&
        html`<div style=${{ fontFamily: T.mono, fontSize: "12px", color: T.t2, whiteSpace: "pre-wrap" }}>
          ${(events || [])
            .filter((evt) => evt.event_type !== "system")
            .map((evt) => `${formatTime(evt)} ${evt.event_type}: ${evt.action}`)
            .join("\n") || "No structured events yet."}
        </div>`}
        ${tab === "terminal" &&
        html`<pre style=${{ margin: 0, whiteSpace: "pre-wrap", fontFamily: T.mono, fontSize: "12px", color: T.t2 }}>
${terminal?.content || "No terminal output yet."}
        </pre>`}
      </div>
    </div>
  </div>`;
}
