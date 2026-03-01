import { html, Badge, Dot } from "./shared.js";
import { T } from "../theme.js";

export function TopNav({ view, onViewChange, pendingCount, connection }) {
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
    <div style=${{ display: "flex", gap: "20px" }}>
      ${tab("fleet", "Dashboard")}
      ${tab("decisions", "Decisions", pendingCount ? html`<${Badge} color=${T.ember} bg=${T.emberS}>${pendingCount}</${Badge}>` : null)}
      ${tab("replay", "Replay")}
      ${tab("attach", "Session")}
    </div>
    <div style=${{ display: "flex", alignItems: "center", gap: "8px", color: T.t2, fontSize: "12px" }}>
      <${Dot} color=${connColor} size=${8} pulse=${connection !== "live"} />
      ${connLabel}
    </div>
  </div>`;
}
