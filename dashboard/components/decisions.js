import { useEffect, useState } from "../lib/hooks.module.js";
import { html, Card, Badge, RepoPill, Dot, severityColor } from "./shared.js";
import { T } from "../theme.js";
import { filterByRepo, formatAge, sortDecisions } from "./decision_utils.js";

function RepoFilterBar({ repos, active, onChange }) {
  return html`<div style=${{ display: "flex", gap: "10px", flexWrap: "wrap" }}>
    <button
      onClick=${() => onChange("all")}
      style=${{
        border: `1px solid ${active === "all" ? T.ember : T.border}`,
        background: active === "all" ? T.emberS : "transparent",
        color: active === "all" ? T.ember : T.t2,
        padding: "6px 10px",
        borderRadius: "999px",
        cursor: "pointer",
      }}
    >All repos</button>
    ${repos.map(
      (repo) => html`<button
        onClick=${() => onChange(repo.id)}
        style=${{
          border: `1px solid ${active === repo.id ? repo.color : T.border}`,
          background: active === repo.id ? `${repo.color}22` : "transparent",
          color: active === repo.id ? repo.color : T.t2,
          padding: "6px 10px",
          borderRadius: "999px",
          cursor: "pointer",
        }}
      >${repo.label}</button>`
    )}
  </div>`;
}

function DecisionContextBlock({ context }) {
  if (!context) return null;
  if (context.type === "diff") {
    return html`<div style=${{ marginTop: "12px", fontFamily: T.mono, fontSize: "12px", color: T.t2 }}>
      <div style=${{ marginBottom: "6px", color: T.t3 }}>${context.file}</div>
      <pre style=${{ margin: 0, whiteSpace: "pre-wrap" }}>
${context.lines
  .map((line) => {
    const prefix = line.type === "add" ? "+" : line.type === "del" ? "-" : " ";
    return `${prefix} ${line.text}`;
  })
  .join("\n")}
      </pre>
    </div>`;
  }
  if (context.type === "log") {
    return html`<div style=${{ marginTop: "12px", fontFamily: T.mono, fontSize: "12px", color: T.t2, whiteSpace: "pre-wrap" }}>
      ${context.text}
    </div>`;
  }
  if (context.type === "screenshot") {
    return html`<div style=${{ marginTop: "12px", fontStyle: "italic", color: T.t3 }}>${context.description}</div>`;
  }
  return null;
}

export function DecisionQueue({ decisions, workspace, reviewer, onAction, onSelectSession, hotkeyAction, loading = false }) {
  const [expandedId, setExpandedId] = useState(null);
  const [repoFilter, setRepoFilter] = useState("all");
  const [commentingId, setCommentingId] = useState(null);
  const [commentText, setCommentText] = useState("");

  const sorted = sortDecisions(decisions || []);
  const filtered = filterByRepo(sorted, repoFilter);

  useEffect(() => {
    if (!hotkeyAction) return;
    if (hotkeyAction.type === "escape") {
      setExpandedId(null);
      setCommentingId(null);
      setCommentText("");
      return;
    }
    const target = filtered[0];
    if (!target) return;
    if (hotkeyAction.type === "approve") {
      onAction(target.id, "approve");
    } else if (hotkeyAction.type === "deny") {
      onAction(target.id, "deny");
    } else if (hotkeyAction.type === "comment") {
      setCommentingId(target.id);
      setExpandedId(target.id);
    }
  }, [hotkeyAction, filtered, onAction]);

  const handleComment = async (id) => {
    if (!commentText.trim()) return;
    await onAction(id, "comment", commentText.trim());
    setCommentText("");
    setCommentingId(null);
  };

  return html`<div style=${{ padding: "28px" }}>
    <div style=${{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
      <div style=${{ fontSize: "20px", fontWeight: 600 }}>Decision Queue</div>
      <div style=${{ color: T.t3, fontSize: "12px" }}>${filtered.length} pending</div>
    </div>

    <div style=${{ margin: "18px 0" }}>
      <${RepoFilterBar} repos=${workspace.repos || []} active=${repoFilter} onChange=${setRepoFilter} />
    </div>

    ${loading && html`<div style=${{ color: T.t3 }}>Loading decisions...</div>`}
    ${!loading && filtered.length === 0 &&
    html`<div style=${{ color: T.t3 }}>No pending decisions.</div>`}

    <div style=${{ display: "grid", gap: "14px" }}>
      ${filtered.map((d) => {
        const expanded = expandedId === d.id;
        const age = formatAge(d.created_at || d.createdAt);
        const sevColor = severityColor(d.severity);
        return html`<${Card} style=${{ borderTop: `3px solid ${sevColor}` }}>
          <div
            onClick=${() => setExpandedId(expanded ? null : d.id)}
            style=${{ cursor: "pointer" }}
          >
            <div style=${{ display: "flex", justifyContent: "space-between" }}>
              <div style=${{ display: "flex", gap: "12px", alignItems: "center" }}>
                <${Dot} color=${sevColor} size=${10} pulse=${d.severity === "critical"} />
                <div>
                  <div style=${{ fontWeight: 600 }}>${d.question}</div>
                  <div style=${{ fontSize: "12px", color: T.t3 }}>
                    <span style=${{ fontFamily: T.mono }}>${d.id}</span> · ${age} · ${d.repo_id}
                    ${d.session_id &&
                    html` · <button
                      onClick=${(e) => {
                        e.stopPropagation();
                        onSelectSession?.(d.session_id);
                      }}
                      style=${{
                        background: "transparent",
                        border: "none",
                        color: T.info,
                        cursor: "pointer",
                        fontSize: "12px",
                        padding: 0,
                        fontFamily: T.mono,
                      }}
                    >${d.session_id}</button>`}
                  </div>
                </div>
              </div>
              <${Badge} color=${sevColor} bg=${T.bg3}>${d.severity}</${Badge}>
            </div>

            <div style=${{ marginTop: "10px", display: "flex", gap: "8px", flexWrap: "wrap" }}>
              <${RepoPill} repoId=${d.repo_id} repos=${workspace.repos || []} />
              ${(d.tags || []).map((tag) => html`<${Badge} color=${T.t2} bg=${T.bg3}>${tag}</${Badge}>`)}
            </div>
          </div>

          <div style=${{ display: "flex", gap: "8px", marginTop: "12px" }}>
            <button
              onClick=${(e) => { e.stopPropagation(); onAction(d.id, "approve"); }}
              style=${{ background: T.okS, color: T.ok, border: `1px solid ${T.ok}`, borderRadius: "10px", padding: "6px 10px", cursor: "pointer" }}
            >Approve</button>
            <button
              onClick=${(e) => { e.stopPropagation(); onAction(d.id, "deny"); }}
              style=${{ background: T.errS, color: T.err, border: `1px solid ${T.err}`, borderRadius: "10px", padding: "6px 10px", cursor: "pointer" }}
            >Deny</button>
            <button
              onClick=${(e) => { e.stopPropagation(); setCommentingId(d.id); }}
              style=${{ background: T.bg3, color: T.t2, border: `1px solid ${T.border}`, borderRadius: "10px", padding: "6px 10px", cursor: "pointer" }}
            >Comment</button>
          </div>

          ${commentingId === d.id && html`<div style=${{ marginTop: "10px" }}>
            <textarea
              value=${commentText}
              onInput=${(e) => setCommentText(e.target.value)}
              placeholder="Add a comment"
              style=${{ width: "100%", minHeight: "70px", background: T.bg1, border: `1px solid ${T.border}`, color: T.t1, borderRadius: "10px", padding: "8px", fontFamily: T.sans }}
            ></textarea>
            <div style=${{ marginTop: "8px", display: "flex", gap: "8px" }}>
              <button
                onClick=${(e) => { e.stopPropagation(); handleComment(d.id); }}
                style=${{ background: T.infoS, color: T.info, border: `1px solid ${T.info}`, borderRadius: "8px", padding: "6px 10px", cursor: "pointer" }}
              >Submit</button>
              <button
                onClick=${(e) => { e.stopPropagation(); setCommentingId(null); setCommentText(""); }}
                style=${{ background: "transparent", color: T.t3, border: `1px solid ${T.border}`, borderRadius: "8px", padding: "6px 10px", cursor: "pointer" }}
              >Cancel</button>
            </div>
          </div>`}

          ${expanded && html`<div style=${{ marginTop: "14px" }}>
            <${DecisionContextBlock} context=${d.context} />
          </div>`}
        </${Card}>`;
      })}
    </div>
  </div>`;
}
