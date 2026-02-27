# Research: Happy Coder

Source: `/home/jonochang/lib/jc/happy`

Happy Coder is an open-source, end-to-end encrypted mobile and web client for
controlling AI coding agents (Claude Code, Codex, Gemini) from anywhere. It is
a TypeScript monorepo with five packages: a React Native mobile/web app, a
Node.js CLI, a remote-control agent CLI, a Fastify backend server, and a shared
wire protocol library.

Both projects solve a similar problem space -- durable, remote management of AI
coding agent sessions -- but take different approaches. Forgemux is
infrastructure-first (tmux, SSH, edge daemons), while Happy is
client-first (mobile app, E2E encryption, zero-knowledge server).

Below are ideas from Happy that could strengthen Forgemux.

---

## 1. End-to-End Encryption

Happy encrypts all session data client-side using TweetNaCl (secretbox + box)
before transmitting to the server. The server is zero-knowledge -- it stores
encrypted blobs it cannot decrypt. This is a strong trust model for remote
session management.

**Relevance to Forgemux:** Session transcripts and agent interactions may
contain secrets (API keys, credentials, proprietary code). Once Forgemux adds
WebSocket-based remote attach and hub aggregation, transit and at-rest
encryption become important. A similar client-side encryption layer would allow
hubs to relay session data without being trusted with content.

**Possible approach:**
- Per-session symmetric key generated on the edge, shared with authorized
  clients via asymmetric key exchange.
- Hub relays encrypted session events without decrypting.
- Transcript files on disk encrypted with a node-local key.

---

## 2. Typed Wire Protocol with Schema Validation

Happy defines a shared `happy-wire` package using Zod schemas for all messages
exchanged between client, CLI, and server. This includes:

- `SessionMessage` -- base message format
- `SessionEnvelope` -- modern event-stream protocol
- `CoreUpdateContainer` -- discriminated unions for update types
- Backward-compatible legacy format with feature flags

Runtime validation catches protocol drift between client and server versions.

**Relevance to Forgemux:** Forgemux currently uses ad-hoc JSON serialization
for session records and HTTP/gRPC endpoints. A formal wire protocol would help
as the WebSocket bridge, dashboard, and multi-edge aggregation mature. Rust
equivalents could use `serde` with enum tagging for discriminated unions, plus
optional runtime schema validation.

**Possible approach:**
- Define a `forgemux-wire` crate (or extend `forgemux-core`) with versioned
  message envelopes.
- Use `serde` tagged enums for event types (session-update, state-change,
  transcript-chunk, input-request).
- Version the protocol so older CLIs can talk to newer hubs.

---

## 3. Push Notifications for Agent Input Requests

Happy sends push notifications (via Expo push service) when an AI agent needs
permission or encounters an error. Users get alerted on their phone and can
respond immediately.

**Relevance to Forgemux:** Forgemux already detects `WaitingInput` state via
prompt regex patterns but the notification hooks are stubbed. Happy validates
that push notifications are a core UX requirement for remote agent management.

**Possible approach:**
- Implement the existing notification hook framework (desktop, webhook, command).
- For mobile/remote scenarios, add webhook targets compatible with services like
  ntfy.sh, Pushover, or Slack incoming webhooks.
- The hub could aggregate `WaitingInput` events and fan out notifications.

---

## 4. Daemon Lifecycle Management

Happy's daemon is more mature than Forgemux's current forged daemon:

- **Exclusive lock file** prevents duplicate daemon instances.
- **State persistence** in `daemon.state.json` survives restarts.
- **Automatic version detection** -- daemon restarts itself when CLI version
  changes (avoids stale daemon after upgrades).
- **Health monitoring** via heartbeat with configurable intervals.
- **Runaway detection** -- identifies and cleans up orphaned processes.

**Relevance to Forgemux:** As forged moves from library-call mode to a proper
long-running daemon, these patterns become important for reliability.

**Possible approach:**
- Add a PID lock file to forged startup (already common in Rust daemons).
- Persist daemon state (registered sessions, edge identity) to a JSON file
  alongside the session store.
- Version-check on CLI connect: if fmux version != forged version, warn or
  trigger daemon restart.
- Add a watchdog that reaps tmux sessions whose agent process has exited.

---

## 5. File Watcher for Agent State Detection

Happy monitors Claude Code's session JSONL files on disk for state changes,
rather than relying solely on PTY output parsing. This provides structured data
about what the agent is doing (tool calls, completions, errors).

**Relevance to Forgemux:** Forgemux currently detects state via regex on
terminal output and idle timers, which is fragile. Reading the agent's own
structured log files would give higher-fidelity state:

- Claude Code writes session data to `~/.claude/projects/` as JSONL.
- Codex CLI may produce similar structured output.

**Possible approach:**
- Add a file watcher (inotify/kqueue) sidecar per session that tails the
  agent's session log.
- Parse structured events (tool invocations, permission requests, errors) for
  richer state than regex alone.
- Feed parsed events into the existing `StateDetector` as additional signals
  alongside PTY output.

---

## 6. Optimistic Concurrency for Distributed State

Happy uses versioned state updates for session metadata and dynamic state.
Each update carries a version number; the server rejects stale writes and
returns current state for retry.

**Relevance to Forgemux:** Once multiple clients (CLI, dashboard, foreman) can
modify session state (e.g., send input, change policy), concurrent writes
become possible. The current file-based session store has no concurrency
control.

**Possible approach:**
- Add a version field to `SessionRecord`.
- On writes, compare-and-swap the version.
- For the hub relay case, the edge is authoritative; hub caches with version
  tracking.

---

## 7. QR Code Authentication for Remote Pairing

Happy uses QR code scanning to pair mobile devices with the CLI/daemon. The QR
encodes the encryption keys needed to establish a secure channel.

**Relevance to Forgemux:** For the future browser-based terminal attach (Phase
3), a QR-based pairing flow could simplify authentication without requiring
users to manage API tokens or SSH keys. Scan a QR from the terminal to
authorize a browser session.

---

## 8. Multi-Agent Abstraction Layer

Happy wraps Claude Code, Codex, and Gemini behind a common `agent` abstraction
with per-agent adapters handling:

- Process spawning and lifecycle
- Input/output protocol differences
- MCP bridge for agents that support Model Context Protocol

**Relevance to Forgemux:** Forgemux already supports multiple agent types via
CLI command templates, but the abstraction is thin. Happy's pattern of
per-agent adapters with a common interface could help as agent-specific features
grow (e.g., reading Claude's JSONL logs vs. Codex's output format).

---

## 9. Real-Time Session Sync with Event Replay

Happy's WebSocket layer supports event replay -- when a client connects
mid-session, it receives buffered events to reconstruct current state. Events
are streamed as typed envelopes.

**Relevance to Forgemux:** Forgemux has a per-session event ring buffer but the
WebSocket bridge is echo-only. Happy validates that event replay on connect is
essential for a good remote-attach UX. The ring buffer design in forged is
already suited for this; the gap is the serialization and streaming protocol.

**Possible approach:**
- On WebSocket connect, replay the ring buffer as typed events.
- Then stream new events in real time.
- Use the wire protocol (idea #2) for event framing.

---

## 10. Structured Transcript Format

Happy stores session interactions as structured message objects (role, content,
tool calls, timestamps) rather than raw terminal output. This enables search,
filtering, and replay in the mobile app.

**Relevance to Forgemux:** Forgemux transcripts are raw PTY output with
shell-based timestamps (`awk strftime`), which is lossy and
locale-dependent. Structured transcripts would enable:

- Searchable session history.
- Token usage extraction.
- Foreman summarization of worker sessions.
- Dashboard replay.

**Possible approach:**
- Supplement (not replace) raw transcripts with a structured event log per
  session.
- Each event: `{ timestamp, type, content }` where type is one of
  `agent-output`, `user-input`, `state-change`, `tool-call`, etc.
- The file watcher (idea #5) could produce these structured events from the
  agent's own logs.

---

## 11. Ephemeral vs Persistent Event Streams

Happy distinguishes durable "update" events (sequenced, replayable, stored) from
transient "ephemeral" events (presence heartbeats, live usage counters, typing
indicators). Ephemeral events are broadcast to connected clients but never
persisted. This keeps storage costs down and avoids replaying stale presence
data on reconnect.

**Relevance to Forgemux:** Forgemux currently treats all events equally -- PTY
output, state changes, and metrics all flow through the same capture path. Once
the WebSocket bridge and dashboard are live, the distinction matters:

- **Durable events** (state transitions, transcript chunks, tool calls, usage
  records) must be persisted, sequenced in the ring buffer, and replayed on
  reconnect. These are the events that reconstruct session state.
- **Ephemeral events** (agent "thinking" indicator, live token counter, cursor
  position, active-user presence) should be broadcast to connected clients but
  excluded from the ring buffer and transcript. Replaying a stale "user X is
  typing" event after a reconnect is confusing; replaying thousands of
  per-second thinking indicators wastes bandwidth.

**Possible approach:**
- Tag each event in the wire protocol with a durability flag (`durable` vs
  `ephemeral`).
- The ring buffer and transcript writer only store durable events.
- The WebSocket bridge broadcasts both, but on `RESUME` replay, skips ephemeral.
- The hub's optional store-and-forward buffer also skips ephemeral events.

This is a small design decision that has outsized impact on storage, replay
performance, and reconnect UX. Easier to build in from the start than to
retrofit after events are already being stored.

---

## 12. Artifact Store for Session Attachments

Happy includes artifacts (file headers, bodies, and per-record encryption keys)
as a first-class sync entity separate from the message stream. This supports
rich attachments -- diffs, log excerpts, screenshots, generated files -- without
bloating the event stream or transcript.

**Relevance to Forgemux:** Several planned features will produce artifacts that
should be associated with a session but don't belong inline in the event stream:

- Foreman supervision reports (structured JSON or markdown).
- Git diffs captured at session milestones.
- Terminal snapshots (the `tmux capture-pane` output from Phase 3).
- Usage summaries exported for cost review.
- Error logs or crash dumps from failed sessions.

Currently, these would either be inlined into the transcript (noisy, hard to
retrieve) or stored as ad-hoc files in the session directory (undiscoverable,
no API access).

**Possible approach:**
- Add an artifact store in the hub API, keyed by session ID and artifact ID.
- Each artifact has metadata (type, timestamp, size) and a blob.
- Events in the session stream reference artifacts by ID rather than inlining
  content.
- The dashboard can render artifacts in a side panel (diff viewer, report
  viewer) without loading the full transcript.
- For E2E encryption scenarios, artifacts are encrypted at the edge with the
  session key, and the hub stores opaque blobs.

**Priority:** Low for MVP. Becomes useful when the Foreman (Phase 5) and
dashboard (Phase 4) need to surface structured outputs.

---

## 13. RPC Surface for Dashboard Inspection

Happy registers a set of remote-callable tools (bash execution, file read/write,
ripgrep search, difftastic diff) over its WebSocket connection, allowing the
mobile app to inspect session state without a full terminal attach.

**Relevance to Forgemux:** Forgemux already has gRPC/HTTP endpoints on forged
for session lifecycle management, and the design specifies `fmux inject` for
command injection via `tmux send-keys`. What it does not have is a structured
inspection API -- a way to query file contents, search code, or view diffs from
the dashboard or Foreman without opening a terminal.

This is distinct from terminal attach. Terminal attach gives full PTY access;
inspection RPCs return structured data for specific queries. The use cases:

- **Dashboard "inspect" panel:** View the current state of files the agent has
  modified, without attaching a terminal and scrolling through output.
- **Foreman automation:** The Foreman currently reads transcript files and runs
  `git diff` inside its own tmux session. Structured RPCs would let it query
  file state directly, reducing the indirection.
- **Troubleshooting:** An operator checks what an agent is doing without
  disrupting the session by attaching.

**Possible approach:**
- Add a small, bounded RPC registry on forged with read-only inspection methods:
  - `file.read(session_id, path)` -- read a file from the session's working
    directory.
  - `file.list(session_id, glob)` -- list files matching a pattern.
  - `git.diff(session_id)` -- current uncommitted changes.
  - `git.log(session_id, n)` -- recent commits.
- Gate access behind the same auth as terminal attach (session owner or
  read-only viewer role).
- Do not include write operations -- command injection via `fmux inject` is the
  existing write path and should remain the only one.

**Priority:** Medium. Most valuable once the dashboard (Phase 4) and Foreman
(Phase 5) need to inspect session working directories.

---

## 14. Standardized Local State Layout with Diagnostics

Happy's CLI stores all local state in a single home directory with predictable
files (`settings.json`, `access.key`, `daemon.state.json`, `logs/`). A user or
support engineer can find everything in one place.

**Relevance to Forgemux:** Forgemux already specifies file locations for the
daemon and CLI:

- CLI config: `~/.config/forgemux/config.toml`
- CLI TLS certs: `~/.config/forgemux/ca.crt`, `client.crt`, `client.key`
- Daemon data: `/var/lib/forgemux/` (configurable via `data_dir`)
- Session records: `<data_dir>/sessions/*.json`
- Transcripts: `<data_dir>/sessions/<id>/transcript.log`

What is not yet specified:
- CLI-side cached state (last-used edge, session history, pairing tokens).
- CLI log output location.
- Daemon PID file and lock file location.
- A diagnostic command that validates the full layout.

`forged check` validates daemon config, certs, tmux, and agent binaries, but
there is no equivalent on the CLI side. An `fmux doctor` command that checks
config syntax, cert validity, hub reachability, and forged version compatibility
would reduce support burden.

**Possible approach:**
- Document the full filesystem layout in a single reference section of the
  design doc.
- Add `fmux doctor` that validates CLI config, TLS chain, hub connectivity,
  and edge version compatibility.
- Standardize CLI cache at `~/.cache/forgemux/` (session history, last-used
  edge) and CLI logs at `~/.local/state/forgemux/logs/` (following XDG
  conventions).

**Priority:** Low. Useful for operational maturity (Phase 9) and multi-user
deployments (Phase 7).

---

## Summary

| Idea | Forgemux Gap | Priority |
|------|-------------|----------|
| E2E encryption | No encryption for remote relay | Medium (needed for hub relay) |
| Typed wire protocol | Ad-hoc JSON serialization | High (foundation for WebSocket, dashboard) |
| Push notifications | Notification hooks are stubbed | High (core UX for WaitingInput) |
| Daemon lifecycle | Basic daemon, no lock/version check | Medium (reliability) |
| File watcher for state | Regex-only state detection | High (state detection fidelity) |
| Optimistic concurrency | No concurrent write protection | Low (needed at multi-client scale) |
| QR auth pairing | No browser auth story | Low (Phase 3+) |
| Multi-agent abstraction | Thin agent adapters | Medium (grows with agent diversity) |
| Event replay on connect | Ring buffer exists, not streamed | High (WebSocket attach UX) |
| Structured transcripts | Raw PTY output only | Medium (enables foreman, search, dashboard) |
| Ephemeral vs persistent events | All events treated equally | High (storage, replay, reconnect UX) |
| Artifact store | No structured session attachments | Low (useful for Foreman, dashboard) |
| RPC inspection surface | No structured file/diff queries | Medium (dashboard, Foreman) |
| Standardized local state | CLI-side layout undocumented | Low (operational maturity) |
