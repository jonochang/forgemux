# Forgemux — Roadmap

**Date:** February 2026
**Status:** Draft

---

## Principles

The roadmap is structured around a single rule: **each phase must be independently useful.** No phase exists solely to enable a future phase. Engineers should get value from the first week of deployment, not after the full vision is delivered.

Phases are scoped by what they unlock, not by component. Each phase may touch all three binaries.

---

## Phase 0 — MVP: Durable Sessions with State Awareness

**Goal:** A single engineer can start, detach, reattach, and terminate agent sessions on an edge node — and know at a glance which sessions need their attention. No hub. No browser. No dashboard.

The core value is twofold: sessions that don't die when your terminal closes, and immediate visibility into which agents are waiting for you.

**What ships:**

| Component | Scope |
|---|---|
| `fmux` | `start`, `attach`, `detach`, `stop`, `kill`, `ls`, `status`, `logs`, `version`, `help` |
| `forged` | `run`, `check`, `sessions`, `health`, `version`, `help` |
| `forgehub` | Not included |
| Dashboard | Not included |

**Capabilities:**

- `fmux start --agent claude` creates a tmux session, launches the agent, starts the sidecar
- `fmux attach <id>` opens an SSH-based tmux attach
- Sessions survive disconnects; `fmux ls` shows what's running and **what needs you**
- `fmux logs <id> --follow` streams the transcript
- `forged check` validates config, tmux, agent binary, and certs before first run
- `forged health` returns JSON for basic monitoring
- Transcript capture to disk (raw terminal bytes with timestamps)
- Session state machine enforced (Provisioning → Running → Idle → **WaitingInput** → Terminated)
- Basic idle timeout policy
- Config via `/etc/forgemux/forged.toml`

**Agent state detection** is the critical differentiator in the MVP. The sidecar must reliably distinguish:

| State | Detection method |
|---|---|
| `Running` | Agent process alive, terminal I/O active |
| `Idle` | Agent process alive, no I/O for configurable threshold |
| `WaitingInput` | Agent has prompted and is blocked on user input |
| `Errored` | Agent process exited nonzero or health check failed |
| `Terminated` | tmux session destroyed |

**Detecting `WaitingInput`** is the hardest and most valuable state. Strategies, in order of preference:

1. **Structured agent log files.** When available, prefer the agent's own structured output over PTY heuristics. Claude Code writes JSONL session data (tool calls, permission requests, completions, errors) to `~/.claude/projects/`. The sidecar watches these files via `inotify`/`kqueue` and parses state-relevant events directly. This is higher fidelity than terminal scraping because it is decoupled from TUI rendering changes. PTY heuristics (below) serve as fallback for agents that lack structured logs.
2. **Cursor position heuristic.** When the agent prints a prompt and the cursor sits at an input line with no output for a threshold period, the session is likely waiting. The sidecar monitors the tmux pane's cursor position and output activity.
3. **Agent-specific prompt patterns.** Each agent CLI has a recognisable prompt string (e.g., Claude Code's `>` prompt, Codex's input marker). The sidecar matches recent terminal output against configurable regex patterns per agent type.
4. **Process tree inspection.** When the agent's child process (the LLM call) has completed and the agent process is blocked on stdin, the session is waiting. Inspect `/proc/<pid>/status` and `/proc/<pid>/wchan`.

The sidecar should use a combination of these — structured logs when the agent provides them, pattern match as primary PTY fallback, cursor + inactivity as secondary, corroborated with process state.

**`fmux ls` output emphasises state:**

```
$ fmux ls
ID       AGENT    MODEL    STATE           AGE     LAST ACTIVITY
S-0a3f   claude   sonnet   ● running       2h      12s ago
S-1b4e   claude   opus     ⏳ waiting      45m     45m ago
S-2c5d   codex    o3       ● running       1h      3s ago
S-3d6e   claude   sonnet   ⚠ errored       3h      2h ago
S-4e7f   claude   haiku    ○ idle          20m     8m ago
```

Sessions in `WaitingInput` sort to the top by default. The engineer's workflow becomes: run `fmux ls`, see what needs them, attach, respond, detach.

**What is explicitly deferred:**

- Hub, dashboard, browser attach
- Token usage parsing and cost tracking
- cgroup/namespace sandboxing
- Multi-node anything
- API tokens, RBAC, user management
- WebSocket bridge
- Push notifications (desktop/Slack/webhook)

**Exit criteria:**

- Engineer can start a Claude Code session, close their laptop, reopen it, and reattach with full context
- `fmux ls` accurately reflects session state, including `WaitingInput`
- An agent that has been waiting for input for 45 minutes shows as `⏳ waiting` — not `running` or `idle`
- Transcript is recoverable after session terminates
- `forged check` catches all common misconfiguration

**Estimated effort:** 5–7 weeks for one engineer. State detection adds ~1–2 weeks over basic session management.

---

## Phase 1 — Notifications: Don't Make Me Poll

**Goal:** Engineers get notified when a session needs their attention, without running `fmux ls` in a loop.

**What ships:**

| Component | Additions |
|---|---|
| `fmux` | `watch` command, `--notify` flag on `start` |
| `forged` | Notification hooks (configurable per session) |

**Capabilities:**

- `fmux watch` — live-updating terminal view of all sessions, highlighting state changes (similar to `watch` or `htop` for sessions)
- `fmux start --notify desktop` — emit a desktop notification when the session enters `WaitingInput` or `Errored`
- Configurable notification hooks in `forged.toml`:
  - `desktop` — OS-native notification (`notify-send` on Linux, `osascript` on macOS)
  - `webhook` — POST to a URL (Slack incoming webhook, Teams, custom)
  - `command` — run an arbitrary command with session metadata as env vars
- Notifications fire on state transitions: `Running → WaitingInput`, `Running → Errored`, `Idle → Terminated` (timeout)
- Debounce: no duplicate notifications for the same state within a configurable window
- Delivery failure handling: webhook notifications retry with exponential backoff (3 attempts over 5 minutes). If the primary channel fails (e.g., desktop notification with no display attached), fall back to the next configured channel. Notification delivery attempts and outcomes are recorded in the session event log so engineers can verify delivery via `fmux status <id>`
- Rate limiting: cap total notifications per session per hour to prevent spam from flapping sessions

**Configuration:**

```toml
[notifications.defaults]
on_waiting_input = ["desktop"]
on_error = ["desktop", "webhook"]
on_idle_timeout = ["webhook"]
debounce = "5m"

[notifications.webhook]
url = "https://hooks.slack.com/services/T.../B.../xxx"
template = '{"text": "Session {{session_id}} ({{agent}}) is {{state}}"}'
```

**Dependencies:** Phase 0.

**Estimated effort:** 2–3 weeks.

---

## Phase 2 — Hub: Multi-Node Visibility

**Goal:** Multiple edge nodes report into a central hub. Engineers and leads can see all sessions across the fleet from one place.

**What ships:**

| Component | Additions |
|---|---|
| `forged` | Hub registration, heartbeat, metrics streaming |
| `forgehub` | `run`, `check`, `edges`, `sessions`, `status`, `db migrate`, `version`, `help` |
| `fmux` | Hub-mediated routing (default path), `edges` command |

**Capabilities:**

- `forged` registers with `forgehub` on startup; maintains persistent gRPC stream
- Hub maintains edge registry with health, capacity, session count
- `fmux ls` without `--edge` queries the hub and returns sessions across all nodes — `WaitingInput` sessions still sort to top
- `fmux edges` shows registered edge nodes and their status
- `fmux start` without `--edge` routes through hub (hub selects edge)
- `forgehub sessions` for aggregate session listing
- Event store (SQLite initially) for session lifecycle events
- `forgehub db migrate` for schema management
- mTLS between edge and hub
- Notifications now include edge node identity

**Machine/edge identity as a first-class entity.** Each edge node is modelled as a distinct machine entity in the hub, separate from the sessions it hosts. The machine record includes: node ID, advertise address, daemon version, capacity, health status, and last heartbeat timestamp. The hub exposes a `machine-heartbeat` endpoint that dashboards and monitoring tools can poll independently of session state. This separation ensures that session routing, dashboard edge health panels, and version tracking have a stable data model to build on.

**Authority model.** The edge is the source of truth for session state. The hub is a cache and aggregator — it reflects what edges report but does not independently mutate session state. Commands routed through the hub (e.g., `fmux stop`) are relayed to the owning edge, which executes them and reports the resulting state change back to the hub.

**Versioned state updates.** Both machine state and session metadata carry a version number. Updates use compare-and-swap semantics: the hub rejects stale writes and returns current state for retry. This prevents silent overwrites when multiple clients (CLI, dashboard, foreman) interact with the same session or when edges reconnect after a network partition.

**Dependencies:** Phase 0, Phase 1.

**Estimated effort:** 4–6 weeks.

---

## Phase 3 — Browser and Mobile Attach: Reliable Stream Protocol

**Goal:** Engineers and reviewers can attach to a running session from a browser or mobile device. Connections are ephemeral; the session is durable. Reconnections are seamless — no lost output, no double-sent input.

**What ships:**

| Component | Additions |
|---|---|
| `forged` | Reliable stream protocol (event ring buffer, snapshots, input dedup), WebSocket bridge |
| `forgehub` | WebSocket relay with optional store-and-forward, JWT auth |
| Dashboard | Minimal SPA: session list + xterm.js terminal with reconnect support |
| `fmux` | `attach --mode web` opens browser |

**Prerequisite deliverable: session event protocol schema.** Before building the WebSocket bridge, define the typed event envelope and ring buffer schema as a shared protocol crate (`forgemux-proto` or equivalent). This schema is consumed by `forged` (event production), `forgehub` (relay and store-and-forward), and the dashboard SPA (rendering). Deferring protocol design until mid-phase risks rework across all three consumers. The schema should be the first deliverable of this phase, covering: event envelope format, message type discriminants, durability classification (durable vs ephemeral — see below), and version negotiation fields.

**Capabilities:**

- Five-message wire protocol: `RESUME`, `EVENT`, `INPUT`, `ACK`, `SNAPSHOT`
- Per-session event ring buffer (8 MB / 30 min default) — output events are sequenced with monotonic IDs and replayable on reconnect
- Client sends `RESUME(last_seen_event_id)` on reconnect; edge replays missed events
- Input deduplication via `input_id` — every keystroke/line is applied exactly once, ACKed explicitly
- Periodic terminal snapshots (via `tmux capture-pane`) for fast catch-up after large gaps
- Hub relay: edge connects outbound to hub, hub exposes stable endpoint for mobile/browser
- Optional hub store-and-forward buffering for brief client disconnects
- Watch mode (read-only, throttled) and Control mode (read-write, full ack/dedupe)
- Adaptive fidelity: keystroke streaming degrades to line-mode under high RTT
- Chunk coalescing and compression for bandwidth efficiency
- Offline command queue (line-mode, guarded) for intermittent connectivity — **disabled by default**, requires explicit enable per session, bounded to 10 queued commands, line-mode only (no raw keystrokes), and restricted to sessions in `WaitingInput` state
- SSH and browser attach coexist on the same session simultaneously
- Events classified as **durable** (state transitions, transcript chunks, tool calls, usage records — stored in ring buffer, persisted, replayed on `RESUME`) or **ephemeral** (typing indicator, live token counter, active-user presence — broadcast to connected clients but excluded from ring buffer, transcript, and replay)

**Protocol invariants:**

| Invariant | Description |
|---|---|
| Monotonic event IDs | Each session has a strictly increasing `event_id` counter. IDs are never reused, even after daemon restart. |
| At-most-once input | Every `INPUT` message carries a `(client_id, input_id)` pair. The edge applies each pair exactly once and returns `ACK`. On reconnect, the client resends unacked inputs; the edge deduplicates. |
| Replay bounds | On `RESUME`, the edge replays events from `last_seen_event_id + 1` if available in the ring buffer. If the event has been evicted, the edge sends the latest `SNAPSHOT` followed by events after the snapshot's `event_id`. |
| Server responsibilities | The edge is the sole writer of `event_id`s and the sole applier of `INPUT` to tmux. The hub relays without modifying IDs or reordering. |
| Client responsibilities | The client tracks `last_seen_event_id` and sends it on `RESUME`. The client retransmits unacked `INPUT` messages on reconnect with the original `input_id`. |

**Ring buffer persistence.** The ring buffer is memory-only during normal operation. On graceful daemon shutdown (`forged drain`), the current ring buffer contents and the last `event_id` per session are flushed to disk. On restart, the daemon reloads the persisted state so that clients can `RESUME` without falling back to a snapshot. The `event_id` counter is always recovered from disk (or set to the last known value + 1) to maintain the monotonic invariant across restarts.

**Incremental ship order within this phase:**

1. Session event protocol schema (shared crate — prerequisite for all below)
2. Event IDs + ring buffer on edge (foundation)
3. `RESUME` handshake (reconnect without losing output)
4. `INPUT` ack + dedupe (reliable input)
5. Chunk coalescing + compression (bandwidth)
6. Periodic snapshots (fast catch-up)
7. Hub tunnel + store-and-forward (mobile resilience)
8. Watch/Control modes + adaptive fidelity (mobile UX)
9. Offline command queue (intermittent connectivity — disabled by default)

**Dependencies:** Phase 2.

**Estimated effort:** 5–7 weeks (expanded from 4–6 to account for the reliable stream layer). The protocol schema (step 1) should be completed in the first week to unblock parallel work on edge, hub, and dashboard.

---

## Phase 4 — Dashboard: Real-Time Observability

**Goal:** Full dashboard with live session state, edge health, and one-click attach — without needing the CLI.

**What ships:**

| Component | Additions |
|---|---|
| Dashboard | Session list with live state, detail view, edge health panel |
| `forgehub` | WebSocket push for real-time dashboard updates, REST endpoints for historical data |

**Capabilities:**

- Dashboard shows all sessions: agent type, model, state, CPU/memory, last activity, attached users
- **`WaitingInput` sessions highlighted prominently** — the dashboard's primary value is answering "which agents need me right now?"
- Live state updates via WebSocket (no polling)
- Session detail view: lifecycle timeline, transcript viewer, state history
- Edge health panel: connected nodes, session count, resource utilisation
- Click-to-attach: open browser terminal from session detail view
- Start session from browser (calls hub API)

**Dependencies:** Phase 3.

**Estimated effort:** 3–4 weeks.

---

## Phase 5 — Foreman Agent: Session Supervision

**Goal:** An automated meta-agent watches running sessions, detects stalls, summarises progress, and (optionally) intervenes — using the same agent CLIs, managed by the same substrate.

**What ships:**

| Component | Additions |
|---|---|
| `forged` | Foreman session lifecycle, prompt templating, transcript read access, intervention dispatch |
| `fmux` | `foreman start`, `foreman status`, `foreman report`, `inject` commands |
| Dashboard | Foreman panel showing supervision reports per session |

**Capabilities:**

- `fmux foreman start` launches a Foreman session on an edge node — a standard Forgemux session running Claude Code or Codex with a supervision-oriented system prompt
- Foreman reads other sessions' transcript files and queries session state from `forged`
- Periodically produces structured reports: per-session status (productive, blocked, looping, idle, errored), current hypothesis, files touched, suggested actions
- `fmux foreman report` prints the latest supervision report
- Three intervention levels, configurable in `forged.toml`:
  - *Advisory* (default): reports only, no cross-session interaction
  - *Assisted*: proposes commands for stalled sessions; engineer approves before injection
  - *Autonomous*: can inject commands and spawn helper sessions without approval
- Stall detection heuristics: repeated errors, rebuilding same files, high token usage with no diffs, circular reasoning
- Auto-summarisation for long-running sessions (executive summary, key files, open questions)
- All Foreman actions logged as lifecycle events with `actor: foreman`
- Foreman's own token usage tracked and bounded

**Dependencies:** Phase 0, Phase 1 (state detection). Benefits from Phase 4 (dashboard) for report display.

**Estimated effort:** 4–6 weeks.

---

## Phase 6 — Sandboxing: Policy and Resource Control

**Goal:** Sessions run inside enforced resource and access boundaries. Admins can define and apply policies.

**What ships:**

| Component | Additions |
|---|---|
| `forged` | cgroup v2 enforcement, network namespace isolation, filesystem bind mounts |
| `fmux` | `--policy` flag on `start` |
| `forgehub` | Policy management endpoints, kill-session relay |

**Capabilities:**

- Named policies in `forged.toml` defining CPU, memory, PID, network, and filesystem limits
- `fmux start --policy restricted` applies a named policy
- cgroup v2 slices per session with enforced limits
- Optional network namespace: agent can be restricted to no network, LAN-only, or specific endpoints
- Filesystem scope via bind mounts: agent sees only the repo directory
- `forgehub kill-session` relays kill command to edge
- `fmux stop` and `fmux kill` distinguish graceful vs. forced termination
- Policy violations logged as events

**Platform note:** cgroup v2 and network namespaces are Linux-only. On macOS edge nodes, policy enforcement is best-effort: filesystem scope can be enforced via `sandbox-exec` or directory-level permissions, but CPU/memory/PID limits and network namespaces are not available. macOS hosts should log a warning at startup if policies requiring Linux-only features are configured, and skip enforcement for those policies rather than failing. This keeps macOS viable as a development edge node without blocking the Linux production path.

**Dependencies:** Phase 2.

**Estimated effort:** 3–4 weeks.

---

## Phase 7 — Access Control: Tokens, Users, RBAC

**Goal:** Multi-user access with proper authentication, authorization, and audit.

**What ships:**

| Component | Additions |
|---|---|
| `forgehub` | `token create/ls/revoke`, `user ls/grant/revoke`, RBAC middleware |
| Dashboard | Login flow, user-scoped views |
| `fmux` | Token-based auth to hub |

**Capabilities:**

- API tokens for CLI and CI access (argon2-hashed, shown once on creation)
- Roles: `viewer` (read-only, observe), `operator` (start/attach/stop), `admin` (full control, user management)
- Sessions scoped by user: `fmux ls --mine` is the default
- Dashboard login via token or SSO (if enterprise integration is in scope)
- All API endpoints enforce RBAC
- Attach permissions: session owner can grant read-only or read-write attach to others
- Full audit log: who did what, when, to which session

**Dependencies:** Phase 2.

**Estimated effort:** 3–4 weeks.

---

## Phase 8 — Token Tracking and Cost Visibility

**Goal:** Engineers and leads can see what agents are costing. Every session has token usage data.

**What ships:**

| Component | Additions |
|---|---|
| `fmux` | `usage` command |
| `forged` | Usage collector framework, per-session usage aggregation |
| `forgehub` | `usage` command, usage endpoints, usage charts in dashboard |
| Dashboard | Usage breakdown views, cost-over-time charts |

**Capabilities:**

- Sidecar tails and parses agent JSONL session logs from disk (reusing the same file watcher infrastructure introduced in Phase 0 for state detection):
  - Claude Code: `~/.config/claude/projects/` (v1.0.30+) or `~/.claude/projects/` (legacy)
  - Codex CLI: `~/.codex/sessions/*.jsonl` (token events available from Sept 2025+)
- Both collectors emit normalised `UsageEvent` records (prompt tokens, completion tokens, cache tokens, cost estimate)
- `fmux usage <session-id>` shows input/output tokens, estimated cost
- `fmux usage --since 24h --by agent` shows aggregate usage
- `forgehub usage --by user --since 7d` for management reporting
- Usage data persisted to event store
- Dashboard shows: cost per session, cost over time, cost by agent/model/edge/user
- Graceful degradation: older Codex sessions missing token fields log a warning and report zero usage
- Configurable usage collector and log paths per agent type in `forged.toml`

**Dependencies:** Phase 2. Benefits from Phase 4 (dashboard) for visualisation, but CLI-only usage works without it.

**Estimated effort:** 3–4 weeks.

---

## Phase 9 — Operational Maturity

**Goal:** Production-grade operations: graceful upgrades, data export, retention policies, and monitoring integration.

**What ships:**

| Component | Additions |
|---|---|
| `forged` | `drain`, `rotate-cert`, config hot-reload |
| `forgehub` | `export`, retention policies, Postgres backend |
| Both | Prometheus metrics endpoint, structured log output |

**Capabilities:**

- `forged drain` for graceful maintenance: stop accepting new sessions, wait for existing to finish, then exit
- `forged rotate-cert` reloads TLS certs without daemon restart
- Config hot-reload via filesystem watcher (policy changes without restart)
- `forgehub export csv --since 30d --type usage` for finance/reporting
- Configurable transcript and event retention with automatic cleanup
- Postgres backend for `forgehub` (multi-instance, production durability)
- `/metrics` endpoint on both daemons for Prometheus scraping
- Structured JSON log output for log aggregation (ELK, Loki, etc.)

**Dependencies:** Phase 6, Phase 7, Phase 8.

**Estimated effort:** 3–4 weeks.

---

## Summary Timeline

```
Phase 0  MVP: sessions + state detection           ██████████████░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░
Phase 1  Notifications                              ░░░░░░░░░░░░░░█████░░░░░░░░░░░░░░░░░░░░░░░░
Phase 2  Hub: multi-node visibility                 ░░░░░░░░░░░░░░░░░░░███████████░░░░░░░░░░░░░░
Phase 3  Browser attach                             ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░█████████░░░░░░
Phase 4  Dashboard                                  ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░████░░░░░░
Phase 5  Foreman agent     (after P1, benefits P4)  ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░████████░
Phase 6  Sandboxing        (parallel after P2)      ░░░░░░░░░░░░░░░░░░░░░░░░░████████░░░░░░░░░░░░
Phase 7  Access control    (parallel after P2)      ░░░░░░░░░░░░░░░░░░░░░░░░░░░░████████░░░░░░░░░
Phase 8  Token tracking    (parallel after P2)      ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░████████░░░░░
Phase 9  Operational maturity                       ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░████
                                                    ──────────────────────────────────────────────
                                                    W1    W6    W11   W16   W21   W26   W31  W36
```

**Total estimated duration:** ~32–42 weeks with one engineer. Phases 5, 6, 7, and 8 can all run in parallel tracks after their dependencies are met, compressing the timeline significantly. With two engineers post-Phase 2, the total compresses to ~22–28 weeks.

---

## What Is Not On This Roadmap

These are deliberately excluded. They may become relevant later but are not planned.

- **Task queuing and scheduling.** Forgemux does not decide when to start sessions. An external scheduler or CI system can call `fmux start`. The Foreman can spawn helper sessions but does not manage a task queue.
- **Custom agent development.** Forgemux wraps existing agent CLIs. Building new agents is out of scope.
- **CI/CD pipeline integration.** Not built in, but `fmux` with `--format json` is scriptable enough to be called from CI. A dedicated integration is not planned.
- **Multi-tenancy.** Forgemux assumes a single organisation. Tenant isolation across organisations is not in scope, but the hub supports org/workspace seeding from config for single-org setups.
- **Mobile access.** The dashboard is responsive but not optimised for mobile. Terminal attach from mobile is not a target.

---

*This roadmap is a plan, not a commitment. Scope and sequencing will adjust as we learn from each phase. The invariant is: each phase ships something useful on its own.*
