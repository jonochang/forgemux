import { html, Badge, Dot } from "./shared.js";
import { T } from "../theme.js";

export function TopNav({
  view,
  onViewChange,
  pendingCount,
  connection,
  hubVersion,
  workspaces = [],
  workspaceId,
  onWorkspaceChange,
  orgLabel,
  workspaceName,
}) {
  const tab = (id, label, badge) => html`<button
    onClick=${() => onViewChange(id)}
    style=${{
      background: "transparent",
      border: "none",
      color: view === id ? T.t1 : T.t3,
      fontSize: "13px",
      letterSpacing: "0.08em",
      textTransform: "uppercase",
      cursor: "pointer",
      display: "flex",
      alignItems: "center",
      gap: "8px",
    }}
  >
    ${label}
    ${badge}
  </button>`;

  const connColor = connection === "live" ? T.ok : connection === "connecting" ? T.warn : T.err;
  const connLabel = connection === "live" ? "live" : connection === "connecting" ? "connecting" : "reconnecting";
  const activeWorkspace = workspaces.find((ws) => ws.id === workspaceId);
  const workspaceLabel = workspaceName || activeWorkspace?.name || workspaceId || "default";
  const showSwitcher = workspaces.length > 1 && onWorkspaceChange;

  return html`<div
    style=${{
      display: "flex",
      alignItems: "center",
      justifyContent: "space-between",
      padding: "20px 28px",
      borderBottom: `1px solid ${T.border}`,
      background: "linear-gradient(90deg, rgba(255,255,255,0.02), transparent)",
    }}
  >
    <div style=${{ display: "flex", alignItems: "center", gap: "16px" }}>
      <div
        style=${{
          width: "28px",
          height: "28px",
          borderRadius: "10px",
          background: `linear-gradient(135deg, ${T.ember}, ${T.purple})`,
        }}
      ></div>
      <div>
        <div style=${{ color: T.t1, fontWeight: 600, letterSpacing: "0.06em" }}>FORGEMUX</div>
        <div style=${{ color: T.t3, fontSize: "12px" }}>Hub Dashboard</div>
      </div>
    </div>
    <div style=${{ display: "flex", alignItems: "center", gap: "12px" }}>
      <div style=${{ display: "flex", flexDirection: "column", gap: "4px" }}>
        <div style=${{ fontSize: "10px", letterSpacing: "0.12em", color: T.t3, textTransform: "uppercase" }}>
          ${orgLabel || "Org"}
        </div>
        ${showSwitcher
          ? html`<select
              value=${workspaceId}
              onChange=${(event) => onWorkspaceChange(event.target.value)}
              style=${{
                background: T.bg2,
                color: T.t1,
                border: `1px solid ${T.border}`,
                borderRadius: "8px",
                padding: "6px 10px",
                fontSize: "12px",
              }}
            >
              ${workspaces.map((ws) => html`<option value=${ws.id}>${ws.name || ws.id}</option>`)}
            </select>`
          : html`<div style=${{
              background: T.bg2,
              color: T.t1,
              border: `1px solid ${T.border}`,
              borderRadius: "8px",
              padding: "6px 10px",
              fontSize: "12px",
              minWidth: "120px",
            }}>${workspaceLabel}</div>`}
      </div>
    </div>
    <div style=${{ display: "flex", gap: "20px" }}>
      ${tab("fleet", "Dashboard")}
      ${tab("decisions", "Decisions", pendingCount ? html`<${Badge} color=${T.ember} bg=${T.emberS}>${pendingCount}</${Badge}>` : null)}
      ${tab("replay", "Replay")}
      ${tab("attach", "Session")}
    </div>
    <div style=${{ display: "flex", alignItems: "center", gap: "10px", color: T.t2, fontSize: "12px" }}>
      <a
        href="/version"
        target="_blank"
        rel="noreferrer"
        style=${{
          color: T.t3,
          textDecoration: "none",
          letterSpacing: "0.04em",
          textTransform: "uppercase",
          fontSize: "10px",
        }}
        title=${hubVersion ? `Forgemux Hub ${hubVersion}` : "Forgemux Hub"}
      >
        about${hubVersion ? ` v${hubVersion}` : ""}
      </a>
      <${Dot} color=${connColor} size=${8} pulse=${connection !== "live"} />
      ${connLabel}
    </div>
  </div>`;
}
