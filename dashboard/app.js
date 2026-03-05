import { h, render } from "./lib/preact.module.js";
import { useEffect, useMemo, useState } from "./lib/hooks.module.js";
import htm from "./lib/htm.module.js";
import { TopNav } from "./components/nav.js";
import { FleetDashboard } from "./components/fleet.js";
import { DecisionQueue } from "./components/decisions.js";
import { SessionReplay } from "./components/replay.js";
import { AttachView } from "./components/attach.js";
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

function normalizeWorkspace(raw, fallback) {
  if (!raw) return fallback;
  const attention = raw.attention_budget || raw.attentionBudget || {};
  return {
    ...fallback,
    ...raw,
    org: raw.org || raw.org_id || fallback.org,
    attentionBudget: {
      used: attention.used || 0,
      total: attention.total || fallback.attentionBudget?.total || 12,
      reset_tz: attention.reset_tz || fallback.attentionBudget?.reset_tz || "UTC",
    },
  };
}

function App() {
  const [view, setView] = useHashRoute();
  const [sessions, setSessions] = useState([]);
  const [decisions, setDecisions] = useState([]);
  const [loadingSessions, setLoadingSessions] = useState(true);
  const [loadingDecisions, setLoadingDecisions] = useState(true);
  const [sessionsError, setSessionsError] = useState(false);
  const [decisionsError, setDecisionsError] = useState(false);
  const [replayError, setReplayError] = useState(false);
  const [connection, setConnection] = useState("connecting");
  const [reviewer, setReviewer] = useState(() => localStorage.getItem("forgemux_reviewer") || "Operator");
  const [replaySessionId, setReplaySessionId] = useState(null);
  const [attachSessionId, setAttachSessionId] = useState(null);
  const [replayEvents, setReplayEvents] = useState([]);
  const [replayDiff, setReplayDiff] = useState(null);
  const [replayTerminal, setReplayTerminal] = useState(null);
  const [replayTab, setReplayTab] = useState("diff");
  const [hotkeyAction, setHotkeyAction] = useState(null);
  const [hubVersion, setHubVersion] = useState(null);
  const [workspaces, setWorkspaces] = useState([]);
  const [workspaceId, setWorkspaceId] = useState(baseWorkspace.id);
  const [workspace, setWorkspace] = useState(baseWorkspace);

  useEffect(() => {
    api
      .sessions(workspaceId)
      .then((data) => {
        setSessions(data);
        setSessionsError(false);
      })
      .catch(() => {
        setSessions([]);
        setSessionsError(true);
      })
      .finally(() => setLoadingSessions(false));
    const stop = connectWS(`/sessions/ws?workspace_id=${encodeURIComponent(workspaceId)}`, {
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
  }, [workspaceId]);

  useEffect(() => {
    api
      .version()
      .then((data) => setHubVersion(data?.version || null))
      .catch(() => setHubVersion(null));
  }, []);

  useEffect(() => {
    api
      .workspaces()
      .then((data) => {
        const list = Array.isArray(data) ? data : [];
        setWorkspaces(list);
        if (list.length > 0) {
          const preferred = list.find((ws) => ws.id === workspaceId) ? workspaceId : list[0].id;
          if (preferred !== workspaceId) setWorkspaceId(preferred);
        }
      })
      .catch(() => {
        setWorkspaces([]);
      });
  }, []);

  useEffect(() => {
    api
      .workspace(workspaceId)
      .then((data) => {
        setWorkspace(normalizeWorkspace(data, baseWorkspace));
      })
      .catch(() => {
        setWorkspace(baseWorkspace);
      });
  }, [workspaceId]);

  useEffect(() => {
    api
      .decisions(workspaceId)
      .then((data) => {
        setDecisions(data);
        setDecisionsError(false);
      })
      .catch(() => {
        setDecisions([]);
        setDecisionsError(true);
      })
      .finally(() => setLoadingDecisions(false));
    const stop = connectWS(`/decisions/ws?workspace_id=${encodeURIComponent(workspaceId)}`, {
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
  }, [workspaceId]);

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
      if (event.key === "4") {
        setView("attach");
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
    setReplayError(false);
    api
      .replayTimeline(replaySessionId)
      .then((data) => setReplayEvents(data.events || []))
      .catch(() => {
        setReplayEvents([]);
        setReplayError(true);
      });
    api
      .replayDiff(replaySessionId)
      .then(setReplayDiff)
      .catch(() => {
        setReplayDiff(null);
        setReplayError(true);
      });
    api
      .replayTerminal(replaySessionId)
      .then(setReplayTerminal)
      .catch(() => {
        setReplayTerminal(null);
        setReplayError(true);
      });
  }, [view, replaySessionId]);

  const selectedSession = useMemo(
    () => sessions.find((session) => session.id === replaySessionId) || sessions[0],
    [sessions, replaySessionId]
  );

  const selectSession = (id) => {
    setReplaySessionId(id);
    setView("replay");
  };

  const attachSession = (id) => {
    setAttachSessionId(id);
    setView("attach");
  };

  const orgLabel = workspace.org || workspace.org_id || baseWorkspace.org;

  return html`<div>
    <${TopNav}
      view=${view}
      onViewChange=${setView}
      pendingCount=${pendingCount}
      connection=${connection}
      hubVersion=${hubVersion}
      workspaces=${workspaces}
      workspaceId=${workspaceId}
      onWorkspaceChange=${setWorkspaceId}
      orgLabel=${orgLabel}
      workspaceName=${workspace.name}
    />
    ${view === "fleet" &&
    html`<${FleetDashboard}
      sessions=${sessions}
      workspace=${workspace}
      onSelectSession=${selectSession}
      onAttachSession=${attachSession}
      loading=${loadingSessions}
      error=${sessionsError}
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
      error=${decisionsError}
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
      error=${replayError}
    />`}
    ${view === "attach" &&
    html`<${AttachView} sessions=${sessions} initialSessionId=${attachSessionId} />`}
  </div>`;
}

render(html`<${App} />`, document.getElementById("app"));
