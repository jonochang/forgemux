import { h, render } from "./lib/preact.module.js";
import { useEffect, useMemo, useState } from "./lib/hooks.module.js";
import htm from "./lib/htm.module.js";
import { TopNav } from "./components/nav.js";
import { FleetDashboard } from "./components/fleet.js";
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
  const [connection, setConnection] = useState("connecting");
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

  const pendingCount = useMemo(() => 0, []);

  return html`<div>
    <${TopNav} view=${view} onViewChange=${setView} pendingCount=${pendingCount} connection=${connection} />
    ${view === "fleet" && html`<${FleetDashboard} sessions=${sessions} workspace=${workspace} />`}
    ${view === "decisions" && html`<div style=${{ padding: "28px", color: "#98968F" }}>Decision queue coming next.</div>`}
    ${view === "replay" && html`<div style=${{ padding: "28px", color: "#98968F" }}>Session replay coming next.</div>`}
  </div>`;
}

render(html`<${App} />`, document.getElementById("app"));
