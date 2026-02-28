import { h, render } from "./lib/preact.module.js";
import { useEffect, useMemo, useState } from "./lib/hooks.module.js";
import htm from "./lib/htm.module.js";
import { TopNav } from "./components/nav.js";
import { FleetDashboard } from "./components/fleet.js";
import { DecisionQueue } from "./components/decisions.js";
import { SessionReplay } from "./components/replay.js";
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
  const [loadingSessions, setLoadingSessions] = useState(true);
  const [loadingDecisions, setLoadingDecisions] = useState(true);
  const [connection, setConnection] = useState("connecting");
  const [reviewer, setReviewer] = useState(() => localStorage.getItem("forgemux_reviewer") || "Operator");
  const [replaySessionId, setReplaySessionId] = useState(null);
  const [replayEvents, setReplayEvents] = useState([]);
  const [replayDiff, setReplayDiff] = useState(null);
  const [replayTerminal, setReplayTerminal] = useState(null);
  const [replayTab, setReplayTab] = useState("diff");
  const [hotkeyAction, setHotkeyAction] = useState(null);
  const workspace = baseWorkspace;

  useEffect(() => {
    api
      .sessions()
      .then(setSessions)
      .catch(() => setSessions([]))
      .finally(() => setLoadingSessions(false));
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
    api
      .decisions(workspace.id)
      .then(setDecisions)
      .catch(() => setDecisions([]))
      .finally(() => setLoadingDecisions(false));
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

  useEffect(() => {
    const handleKey = (event) => {
      const tag = document.activeElement?.tagName?.toLowerCase();
      if (tag === "input" || tag === "textarea") return;
      if (event.key === "1") {
        setView("fleet");
        return;
      }
      if (event.key === "2") {
        setView("decisions");
        return;
      }
      if (event.key === "3") {
        setView("replay");
        return;
      }
      if (view === "decisions") {
        if (event.key === "a") setHotkeyAction({ type: "approve", ts: Date.now() });
        if (event.key === "d") setHotkeyAction({ type: "deny", ts: Date.now() });
        if (event.key === "c") setHotkeyAction({ type: "comment", ts: Date.now() });
        if (event.key === "Escape") setHotkeyAction({ type: "escape", ts: Date.now() });
      }
    };
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  }, [view, setView]);

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

  useEffect(() => {
    if (!replaySessionId && sessions.length > 0) {
      setReplaySessionId(sessions[0].id);
    }
  }, [replaySessionId, sessions]);

  useEffect(() => {
    if (view !== "replay" || !replaySessionId) return;
    api
      .replayTimeline(replaySessionId)
      .then((data) => setReplayEvents(data.events || []))
      .catch(() => setReplayEvents([]));
    api
      .replayDiff(replaySessionId)
      .then(setReplayDiff)
      .catch(() => setReplayDiff(null));
    api
      .replayTerminal(replaySessionId)
      .then(setReplayTerminal)
      .catch(() => setReplayTerminal(null));
  }, [view, replaySessionId]);

  const selectedSession = useMemo(
    () => sessions.find((session) => session.id === replaySessionId) || sessions[0],
    [sessions, replaySessionId]
  );

  const selectSession = (id) => {
    setReplaySessionId(id);
    setView("replay");
  };

  return html`<div>
    <${TopNav} view=${view} onViewChange=${setView} pendingCount=${pendingCount} connection=${connection} />
    ${view === "fleet" &&
    html`<${FleetDashboard}
      sessions=${sessions}
      workspace=${workspace}
      onSelectSession=${selectSession}
      loading=${loadingSessions}
    />`}
    ${view === "decisions" &&
    html`<${DecisionQueue}
      decisions=${decisions}
      workspace=${workspace}
      reviewer=${reviewer}
      onAction=${handleDecisionAction}
      onSelectSession=${selectSession}
      hotkeyAction=${hotkeyAction}
      loading=${loadingDecisions}
    />`}
    ${view === "replay" &&
    html`<${SessionReplay}
      session=${selectedSession}
      workspace=${workspace}
      events=${replayEvents}
      tab=${replayTab}
      onTabChange=${setReplayTab}
      diff=${replayDiff}
      terminal=${replayTerminal}
    />`}
  </div>`;
}

render(html`<${App} />`, document.getElementById("app"));
