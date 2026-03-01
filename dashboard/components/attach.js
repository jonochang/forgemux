import { useEffect, useRef, useState, useCallback } from "../lib/hooks.module.js";
import { html, Dot, Badge, SectionLabel, statusColor } from "./shared.js";
import { T } from "../theme.js";
import { ansiToHtml } from "../lib/ansi.js";

export function AttachView({ sessions, initialSessionId }) {
  const [selectedId, setSelectedId] = useState(initialSessionId || null);
  const [content, setContent] = useState("");
  const [attachStatus, setAttachStatus] = useState("disconnected");
  const [inputText, setInputText] = useState("");
  const wsRef = useRef(null);
  const sessionIdRef = useRef(null);
  const pendingInputsRef = useRef([]);
  const terminalRef = useRef(null);

  const flushPending = useCallback(() => {
    const ws = wsRef.current;
    if (!ws || ws.readyState !== WebSocket.OPEN) return;
    const pending = pendingInputsRef.current;
    if (!pending.length) return;
    pending.forEach((payload) => ws.send(payload));
    pendingInputsRef.current = [];
  }, []);

  const selectSession = useCallback(
    (id) => {
      if (wsRef.current) {
        wsRef.current.close();
        wsRef.current = null;
      }

      sessionIdRef.current = id;
      setSelectedId(id);
      setContent("");
      setAttachStatus("connecting");

      if (!id) return;

      const protocol = window.location.protocol === "https:" ? "wss" : "ws";
      const url = `${protocol}://${window.location.host}/sessions/${encodeURIComponent(id)}/attach`;
      const ws = new WebSocket(url);
      wsRef.current = ws;

      ws.addEventListener("open", () => {
        if (sessionIdRef.current !== id) {
          ws.close();
          return;
        }
        setAttachStatus("attached");
        ws.send(JSON.stringify({ type: "resume", last_seen_event_id: 0 }));
        flushPending();
      });

      ws.addEventListener("message", (event) => {
        if (sessionIdRef.current !== id) return;
        try {
          const payload = JSON.parse(event.data);
          if (payload.type === "snapshot" || payload.type === "event") {
            const data = payload.data || "";
            const trimmed = data.replace(/^(\s*\n)+/, "").replace(/(\n\s*)+$/, "");
            setContent(trimmed);
          }
          if (payload.type === "ack") {
            setAttachStatus(`acked ${payload.input_id}`);
          }
        } catch {
          // raw text fallback
          setContent(event.data);
        }
      });

      ws.addEventListener("close", () => {
        if (sessionIdRef.current !== id) return;
        setAttachStatus("disconnected");
        setTimeout(() => {
          if (sessionIdRef.current === id) {
            selectSession(id);
          }
        }, 1500);
      });
    },
    [flushPending]
  );

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      sessionIdRef.current = null;
      if (wsRef.current) {
        wsRef.current.close();
        wsRef.current = null;
      }
    };
  }, []);

  // Auto-scroll terminal
  useEffect(() => {
    if (terminalRef.current) {
      terminalRef.current.scrollTop = terminalRef.current.scrollHeight;
    }
  }, [content]);

  const sendInput = useCallback(() => {
    if (!inputText) return;
    const inputId = Math.random().toString(36).slice(2);
    const payload = JSON.stringify({
      type: "input",
      input_id: inputId,
      data: inputText + "\n",
    });
    const ws = wsRef.current;
    if (!ws || ws.readyState !== WebSocket.OPEN) {
      pendingInputsRef.current.push(payload);
      setAttachStatus(`queued ${inputId}`);
    } else {
      ws.send(payload);
      setAttachStatus(`sent ${inputId}`);
    }
    setInputText("");
  }, [inputText]);

  const handleKeyDown = useCallback(
    (e) => {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        sendInput();
      }
    },
    [sendInput]
  );

  const statusDotColor =
    attachStatus === "attached"
      ? T.ok
      : attachStatus === "connecting"
        ? T.warn
        : T.t4;

  return html`<div style=${{ display: "flex", height: "calc(100vh - 72px)", overflow: "hidden" }}>
    <!-- Session sidebar -->
    <div
      style=${{
        width: "280px",
        borderRight: `1px solid ${T.border}`,
        background: T.bg1,
        display: "flex",
        flexDirection: "column",
      }}
    >
      <div style=${{ padding: "12px 16px", borderBottom: `1px solid ${T.border}` }}>
        <${SectionLabel}>Sessions</${SectionLabel}>
      </div>
      <div style=${{ flex: 1, overflowY: "auto" }}>
        ${sessions.length === 0 &&
        html`<div style=${{ padding: "16px", color: T.t3, fontSize: "13px" }}>No sessions.</div>`}
        ${sessions.map(
          (s) => html`<div
            key=${s.id}
            onClick=${() => selectSession(s.id)}
            style=${{
              padding: "10px 16px",
              cursor: "pointer",
              background: selectedId === s.id ? T.bg3 : "transparent",
              borderLeft: selectedId === s.id ? `3px solid ${T.ember}` : "3px solid transparent",
              display: "flex",
              flexDirection: "column",
              gap: "4px",
            }}
          >
            <div style=${{ display: "flex", alignItems: "center", gap: "8px" }}>
              <${Dot} color=${statusColor(s.state)} size=${7} />
              <span
                style=${{
                  fontSize: "12px",
                  color: T.t2,
                  fontFamily: T.mono,
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                  flex: 1,
                }}
              >${s.goal || s.id}</span>
            </div>
            <div style=${{ display: "flex", alignItems: "center", gap: "6px", paddingLeft: "15px" }}>
              ${s.model &&
              html`<span style=${{ fontSize: "10px", color: T.t3 }}>${s.model}</span>`}
              <${Badge} color=${statusColor(s.state)} bg=${T.bg4}>${s.state || "unknown"}</${Badge}>
            </div>
          </div>`
        )}
      </div>
    </div>

    <!-- Terminal pane -->
    <div style=${{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden" }}>
      <!-- Status bar -->
      <div
        style=${{
          display: "flex",
          alignItems: "center",
          gap: "8px",
          padding: "10px 16px",
          borderBottom: `1px solid ${T.border}`,
          background: T.bg2,
          fontSize: "12px",
          color: T.t2,
        }}
      >
        <${Dot} color=${statusDotColor} size=${7} />
        <span>${attachStatus}</span>
        ${selectedId &&
        html`<span style=${{ fontFamily: T.mono, color: T.t3, marginLeft: "auto" }}>${selectedId}</span>`}
      </div>

      <!-- Terminal output -->
      <div
        ref=${terminalRef}
        style=${{
          flex: 1,
          overflowY: "auto",
          padding: "16px",
          background: T.bg0,
          fontFamily: T.mono,
          fontSize: "12px",
          lineHeight: "1.5",
          color: T.t2,
          whiteSpace: "pre-wrap",
          wordBreak: "break-all",
        }}
        dangerouslySetInnerHTML=${{ __html: content ? ansiToHtml(content) : "Select a session to attach." }}
      ></div>

      <!-- Input area -->
      <div
        style=${{
          display: "flex",
          gap: "8px",
          padding: "10px 16px",
          borderTop: `1px solid ${T.border}`,
          background: T.bg2,
        }}
      >
        <textarea
          value=${inputText}
          onInput=${(e) => setInputText(e.target.value)}
          onKeyDown=${handleKeyDown}
          placeholder=${selectedId ? "Type input... (Enter to send, Shift+Enter for newline)" : "Select a session first"}
          disabled=${!selectedId}
          rows="1"
          style=${{
            flex: 1,
            background: T.bg1,
            border: `1px solid ${T.border}`,
            borderRadius: "6px",
            color: T.t1,
            fontFamily: T.mono,
            fontSize: "12px",
            padding: "8px 10px",
            resize: "none",
            outline: "none",
            minHeight: "36px",
            maxHeight: "120px",
          }}
        />
        <button
          onClick=${sendInput}
          disabled=${!selectedId || !inputText}
          style=${{
            background: T.ember,
            color: "#fff",
            border: "none",
            borderRadius: "6px",
            padding: "8px 16px",
            fontSize: "12px",
            cursor: selectedId && inputText ? "pointer" : "default",
            opacity: selectedId && inputText ? 1 : 0.4,
          }}
        >
          Send
        </button>
      </div>
    </div>
  </div>`;
}
