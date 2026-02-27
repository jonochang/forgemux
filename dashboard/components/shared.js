import { h } from "../lib/preact.module.js";
import htm from "../lib/htm.module.js";
import { T } from "../theme.js";

export const html = htm.bind(h);

export function Dot({ color = T.t2, size = 8, pulse = false }) {
  return html`<span
    style=${{
      width: `${size}px`,
      height: `${size}px`,
      borderRadius: "999px",
      background: color,
      boxShadow: pulse ? `0 0 0 6px ${color}33` : "none",
      display: "inline-block",
    }}
  ></span>`;
}

export function Badge({ children, color = T.t1, bg = T.bg3 }) {
  return html`<span
    style=${{
      background: bg,
      color,
      padding: "2px 8px",
      borderRadius: "999px",
      fontSize: "12px",
      letterSpacing: "0.02em",
    }}
  >${children}</span>`;
}

export function RepoPill({ repoId, repos }) {
  const repo = repos.find((r) => r.id === repoId) || {
    label: repoId,
    color: T.t3,
    icon: "•",
  };
  return html`<span
    style=${{
      display: "inline-flex",
      alignItems: "center",
      gap: "6px",
      border: `1px solid ${T.border}`,
      background: T.bg2,
      padding: "4px 8px",
      borderRadius: "999px",
      fontSize: "12px",
      color: T.t2,
    }}
  ><span style=${{ color: repo.color }}>${repo.icon}</span>${repo.label}</span>`;
}

export function MiniBar({ value = 0, max = 100, color = T.ok, h = 6 }) {
  const pct = max > 0 ? Math.min(100, Math.round((value / max) * 100)) : 0;
  return html`<div
    style=${{
      height: `${h}px`,
      background: T.bg4,
      borderRadius: "999px",
      overflow: "hidden",
    }}
  >
    <div
      style=${{
        width: `${pct}%`,
        height: "100%",
        background: color,
      }}
    ></div>
  </div>`;
}

export function SectionLabel({ children }) {
  return html`<div style=${{ color: T.t3, fontSize: "12px", letterSpacing: "0.12em", textTransform: "uppercase" }}>
    ${children}
  </div>`;
}

export function Card({ children, style = {} }) {
  return html`<div
    style=${{
      background: T.bg2,
      border: `1px solid ${T.border}`,
      borderRadius: "16px",
      padding: "16px",
      boxShadow: "0 10px 30px rgba(0,0,0,0.35)",
      ...style,
    }}
  >${children}</div>`;
}

export function riskColor(risk) {
  switch (risk) {
    case "red":
      return T.err;
    case "yellow":
      return T.warn;
    default:
      return T.ok;
  }
}

export function statusColor(status) {
  switch (status) {
    case "unreachable":
      return T.t4;
    case "errored":
      return T.err;
    case "waiting":
      return T.warn;
    case "idle":
      return T.info;
    default:
      return T.ok;
  }
}

export function contextColor(pct) {
  if (pct >= 85) return T.err;
  if (pct >= 70) return T.warn;
  return T.ok;
}

export function severityColor(sev) {
  switch (sev) {
    case "critical":
      return T.err;
    case "high":
      return T.molten;
    case "medium":
      return T.warn;
    default:
      return T.info;
  }
}
