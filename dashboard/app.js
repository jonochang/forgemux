import { h, render } from "./lib/preact.module.js";
import { useEffect, useMemo, useState } from "./lib/hooks.module.js";
import htm from "./lib/htm.module.js";
import { TopNav } from "./components/nav.js";
import { FleetDashboard } from "./components/fleet.js";
import { DecisionQueue } from "./components/decisions.js";
import { workspace as baseWorkspace } from "./state.js";
import { api } from "./services/api.js";
import { connectWS } from "./services/ws.js";

const html = htm.bind(h);

function useHashRoute(defaultView = "fleet") {
  const [view, setView] = useState(() => window.location.hash.replace("#", "") || defaultView);
  useEffect(() => {
    const onHash = () => setView(window.location.hash.replace("#", "") || defaultView);
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);
  const update = (next) => {
    window.location.hash = next;
  };
  return [view, update];
}

function App() {
  const [view, setView] = useHashRoute();
  const [sessions, setSessions] = useState([]);
  const [decisions, setDecisions] = useState([]);
  const [connection, setConnection] = useState("connecting");
  const [reviewer, setReviewer] = useState(() => localStorage.getItem("forgemux_reviewer") || "Operator");
  const workspace = baseWorkspace;

  useEffect(() => {
    api.sessions().then(setSessions).catch(() => setSessions([]));
    const stop = connectWS("/sessions/ws", {
      onMessage: (data) => {
        try {
          const parsed = JSON.parse(data);
          if (Array.isArray(parsed)) setSessions(parsed);
        } catch {
          // ignore
        }
      },
      onStatus: setConnection,
    });
    return () => stop();
  }, []);

  useEffect(() => {
    api.decisions(workspace.id).then(setDecisions).catch(() => setDecisions([]));
    const stop = connectWS(`/decisions/ws?workspace_id=${encodeURIComponent(workspace.id)}`, {
      onMessage: (data) => {
        try {
          const parsed = JSON.parse(data);
          if (parsed?.type === "decisions_init") {
            setDecisions(parsed.decisions || []);
          } else if (parsed?.type === "decision_created" && parsed.decision) {
            setDecisions((current) => {
              if (current.some((d) => d.id === parsed.decision.id)) return current;
              return [parsed.decision, ...current];
            });
          } else if (parsed?.type === "decision_resolved" && parsed.decision_id) {
            setDecisions((current) => current.filter((d) => d.id !== parsed.decision_id));
          }
        } catch {
          // ignore
        }
      },
    });
    return () => stop();
  }, [workspace.id]);

  useEffect(() => {
    localStorage.setItem("forgemux_reviewer", reviewer);
  }, [reviewer]);

  const pendingCount = useMemo(() => decisions.length, [decisions.length]);

  const handleDecisionAction = async (decisionId, action, comment) => {
    const payload = { reviewer, comment };
    try {
      if (action === "approve") {
        await api.approveDecision(decisionId, payload);
        setDecisions((current) => current.filter((d) => d.id !== decisionId));
      } else if (action === "deny") {
        await api.denyDecision(decisionId, payload);
        setDecisions((current) => current.filter((d) => d.id !== decisionId));
      } else if (action === "comment") {
        await api.commentDecision(decisionId, payload);
      }
    } catch {
      // ignore for now
    }
  };

  return html`<div>
    <${TopNav} view=${view} onViewChange=${setView} pendingCount=${pendingCount} connection=${connection} />
    ${view === "fleet" && html`<${FleetDashboard} sessions=${sessions} workspace=${workspace} />`}
    ${view === "decisions" &&
    html`<${DecisionQueue}
      decisions=${decisions}
      workspace=${workspace}
      reviewer=${reviewer}
      onAction=${handleDecisionAction}
    />`}
    ${view === "replay" && html`<div style=${{ padding: "28px", color: "#98968F" }}>Session replay coming next.</div>`}
  </div>`;
}

render(html`<${App} />`, document.getElementById("app"));
