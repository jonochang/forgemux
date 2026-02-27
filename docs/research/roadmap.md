# Research: Roadmap Recommendations

**Date:** February 2026
**Based on:** Review of Happy Coder (`/home/jonochang/lib/jc/happy`), forgemux
specs, and current implementation (v0.1.5).

This document proposes additions and adjustments to the forgemux roadmap,
informed by patterns observed in the Happy Coder project and gaps identified in
the current design.

See also: [docs/research/happy.md](happy.md) for the raw research on Happy.

---

## 1. Agent Log File Watching for State Detection

**Phase:** 0 (MVP)
**Current gap:** State detection relies on PTY regex matching and idle timers.
The roadmap lists cursor position heuristics and `/proc` inspection as future
signals, but omits agent log file watching entirely.

**Recommendation:** Add agent JSONL log watching as a primary state detection
signal. Claude Code writes structured session data (tool calls, permission
requests, completions, errors) to `~/.claude/projects/` as JSONL. This data is
more reliable than regex on rendered terminal output because it is structured,
versioned, and decoupled from TUI rendering changes.

Happy Coder uses this exact approach -- watching Claude's session files via
filesystem events rather than parsing PTY output.

**What this enables:**
- Detect `WaitingInput` from structured permission-request events rather than
  regex on a rendered prompt string.
- Detect `Errored` from structured error events rather than nonzero exit codes.
- Detect tool call activity (file writes, command execution) for richer Foreman
  reports later.
- Reduce false-positive state transitions caused by TUI rendering artifacts.

**Implementation:**
- Add an `inotify`/`kqueue` watcher per session targeting the agent's log
  directory (already identified in `forged.toml` under `usage_paths`).
- Parse new JSONL lines for state-relevant events.
- Feed parsed signals into `StateDetector` alongside existing PTY/idle signals.
- The same watcher can later feed the usage collector (Phase 8), avoiding
  duplicate file watching infrastructure.

**Effort delta:** +1 week in Phase 0. Reduces risk on the hardest Phase 0
deliverable (WaitingInput accuracy).

---

## 2. Daemon Lifecycle Hardening

**Phase:** 0 (MVP)
**Current gap:** `forged` has no lock file, no version negotiation, no orphan
cleanup. The roadmap defers daemon robustness to later phases implicitly.

**Recommendation:** Add basic daemon lifecycle management to Phase 0, since
forged is a long-running process from day one:

- **PID lock file** at startup (`/var/run/forgemux/forged.pid`). Prevents
  duplicate daemons after accidental double-start or failed shutdown.
- **Version header on CLI connect.** When `fmux` connects to `forged`, include
  the CLI version in the handshake. If there is a major version mismatch, warn
  the user. This avoids subtle protocol drift after upgrades.
- **Orphan tmux session cleanup.** On startup, scan for tmux sessions matching
  the forgemux naming convention that have no corresponding session record.
  These are orphans from a previous crash. Log them and optionally reap them.
- **Daemon state persistence.** Persist the session map and edge identity to a
  JSON file on mutation. On restart, reload and reconcile with actual tmux
  state. This is already partially implemented in the session store but could
  be made more explicit.

Happy's daemon does all of the above plus automatic version-triggered restart.
The full auto-restart pattern is likely overkill for forgemux (systemd handles
restarts), but the lock file and version check are cheap and prevent real
operational issues.

**Effort delta:** +2-3 days in Phase 0.

---

## 3. Structured Event Log Alongside Raw Transcripts

**Phase:** 0-1 (start in MVP, mature in later phases)
**Current gap:** Transcripts are raw PTY bytes with `awk strftime` timestamps.
The design doc acknowledges this is locale-dependent and notes the open question
about transcript format.

**Recommendation:** Write a structured event log per session in parallel with
the raw transcript. Each event is a JSON line:

```jsonl
{"ts":"2026-02-27T10:15:32Z","type":"state-change","data":{"from":"Running","to":"WaitingInput"}}
{"ts":"2026-02-27T10:15:33Z","type":"agent-output","data":{"bytes":1024}}
{"ts":"2026-02-27T10:16:01Z","type":"user-input","data":{"bytes":42,"source":"ssh"}}
{"ts":"2026-02-27T10:16:01Z","type":"tool-call","data":{"tool":"write","file":"src/main.rs"}}
```

The raw transcript remains the source of truth for terminal replay. The event
log is a lightweight index that enables:

- Fast scan by the Foreman (Phase 5) -- read the event log instead of parsing
  megabytes of raw PTY output.
- Dashboard search and filtering (Phase 4).
- Usage attribution -- correlate token events with tool calls.
- Debugging state detection issues -- the event log records what the detector
  saw and when.

If agent log file watching (idea #1) is implemented, the structured events from
the agent's own JSONL feed directly into this event log.

**Effort delta:** +3-4 days in Phase 0 for the basic framework. Enrichment
grows naturally as more signal sources are added.

---

## 4. Wire Protocol Versioning

**Phase:** 2-3 (Hub and Browser Attach)
**Current gap:** The design doc defines protobuf types in `forgemux-proto` but
does not address protocol evolution or backward compatibility.

**Recommendation:** Add explicit protocol versioning from the first hub release:

- Include a `protocol_version` field in the gRPC registration handshake and
  WebSocket RESUME message.
- Define a compatibility policy: hub must support the current version and one
  prior version. Older clients get a clear error with upgrade instructions.
- Use protobuf's built-in field presence for additive changes (new optional
  fields are backward-compatible).
- For breaking changes, increment the version and maintain a brief compatibility
  window.

Happy learned this the hard way -- they have both a legacy and modern session
protocol format, with feature flags to control which path is active. Designing
versioning in from the start avoids this dual-path complexity.

**Effort delta:** Minimal if done at Phase 2 design time. Becomes expensive to
retrofit later.

---

## 5. Reconsider Mobile Access

**Phase:** 3 (Browser Attach)
**Current gap:** The roadmap explicitly excludes mobile access: "Terminal attach
from mobile is not a target."

**Recommendation:** Revisit this exclusion. Happy's entire value proposition is
mobile access to coding agents, and it has found real user demand. The reliable
stream protocol already designed for Phase 3 handles the hard problems
(reconnection, bandwidth, adaptive fidelity, watch/control modes). Mobile is a
natural extension of the browser attach work rather than a separate effort.

Specifically:
- The Phase 3 watch mode (read-only, throttled) is already mobile-friendly by
  design.
- Push notifications (Phase 1) plus mobile watch mode covers the primary mobile
  use case: get notified that a session needs attention, open the dashboard on
  your phone, see what the agent is doing, optionally send a response.
- Full terminal control from mobile is less valuable (typing on a phone is
  painful), but watch mode with single-action responses (approve/deny, pick
  from options) is highly useful for permission prompts.

**Concrete adjustment:** In Phase 3, add a responsive layout to the dashboard
SPA and test the watch mode flow on mobile Safari/Chrome. In Phase 4, add a
simplified mobile view that shows session state + notification response actions
without a full terminal. This is incremental, not a new phase.

**Effort delta:** +1 week in Phase 3-4 if the dashboard is responsive from the
start.

---

## 6. E2E Encryption for Hub Relay

**Phase:** 3 (Browser Attach)
**Current gap:** The design specifies mTLS for transport encryption between edge
and hub, but the hub can see all session content in plaintext. The hub is a
trusted intermediary.

**Recommendation:** Add optional end-to-end encryption for session content
relayed through the hub, so the hub operates as a zero-knowledge relay. This
matches Happy's architecture where the server stores encrypted blobs it cannot
decrypt.

**Why it matters for Forgemux:**
- Session transcripts contain proprietary code, credentials, and agent
  reasoning. If the hub is compromised, everything is exposed.
- For organisations where the hub runs on shared infrastructure (not the same
  trust boundary as edge nodes), zero-knowledge relay is a meaningful security
  improvement.
- It aligns with the existing design principle: "Credentials remain on edge;
  never transmitted to dashboard or browser."

**Implementation sketch:**
- Per-session symmetric key generated on the edge at session start.
- Key shared with authorized clients via asymmetric key exchange (client public
  key registered at auth time).
- All EVENT, SNAPSHOT, and INPUT messages encrypted with the session key before
  leaving the edge.
- Hub relays encrypted payloads without decryption.
- Unencrypted metadata (session ID, state, timestamps) remains visible to the
  hub for routing and dashboard display.

**Effort delta:** +1-2 weeks in Phase 3. Can be made optional (off by default)
to avoid blocking the phase.

---

## 7. Notification Delivery Reliability

**Phase:** 1 (Notifications)
**Current gap:** Phase 1 describes notification hooks (desktop, webhook,
command) with debounce, but does not address delivery guarantees or failure
handling.

**Recommendation:** Add basic reliability to the notification framework:

- **Retry with backoff** for webhook delivery failures (network errors, 5xx
  responses). Cap at 3 retries over 5 minutes.
- **Multi-channel fallback.** If the primary channel fails (e.g., desktop
  notification when no display is attached), fall back to the next configured
  channel.
- **Delivery log.** Record notification attempts and outcomes in the session
  event log. Engineers can check `fmux status <id>` to see whether a
  notification was delivered.
- **Rate limiting per session.** Beyond the debounce window, cap total
  notifications per session per hour. A flapping session (oscillating between
  Running and WaitingInput) should not spam the engineer.

Happy sends push notifications via Expo's push service, which handles retry and
delivery receipts. Forgemux doesn't need a push service, but the webhook path
should be similarly reliable since it may be the only way an engineer learns a
session needs attention.

**Effort delta:** +2-3 days in Phase 1.

---

## 8. Agent Adapter Trait

**Phase:** 0 (MVP, foundational)
**Current gap:** Agent types are configured via command templates in TOML
(`command`, `args`, `env`). Agent-specific behaviour (state detection patterns,
log file locations, usage collection) is spread across separate config fields
and future collector modules.

**Recommendation:** Define an explicit `AgentAdapter` trait that encapsulates
all agent-specific behaviour behind a common interface:

```rust
trait AgentAdapter {
    /// Command and environment to spawn the agent.
    fn spawn_config(&self) -> SpawnConfig;

    /// Regex patterns for WaitingInput detection from PTY output.
    fn prompt_patterns(&self) -> &[Regex];

    /// Paths to the agent's structured log files for a given working directory.
    fn log_paths(&self, cwd: &Path) -> Vec<PathBuf>;

    /// Parse a JSONL line from the agent's log into a structured event.
    fn parse_log_event(&self, line: &str) -> Option<AgentEvent>;

    /// Parse a JSONL line for token usage data.
    fn parse_usage_event(&self, line: &str) -> Option<UsageEvent>;
}
```

This consolidates agent-specific logic into one place per agent type. Happy does
this with per-agent adapter modules (`claude/`, `codex/`, `gemini/`) behind a
common interface. The benefit grows as forgemux adds more agent types or as
existing agents change their output formats.

**Effort delta:** +2-3 days in Phase 0. Simplifies Phase 8 (usage tracking)
and Phase 5 (Foreman, which needs to understand agent output).

---

## 9. Session Handoff Workflow

**Phase:** 4-7 (Dashboard + Access Control)
**Current gap:** The product brief mentions session sharing and the roadmap
mentions attach permissions, but there is no explicit handoff workflow -- how an
engineer transfers ownership of a session to a colleague.

**Recommendation:** Define a lightweight handoff mechanism:

- `fmux transfer <session-id> <user>` transfers session ownership.
  The new owner receives a notification. The previous owner retains read-only
  access by default.
- On the dashboard, a "Transfer" button on the session detail view.
- Transfer is logged as a lifecycle event.
- Optional: transfer with context -- the engineer can attach a message ("I got
  the auth refactor to compile but the tests are failing, take a look at
  test_login").

This matters because agent sessions accumulate context that is expensive to
rebuild. If an engineer goes off-shift or hits a wall, transferring the live
session (with full transcript and agent context) to a colleague is far more
efficient than starting a new session.

**Effort delta:** +3-4 days, best placed in Phase 7 (Access Control) since it
depends on user identity.

---

## 10. CLI-Daemon Version Compatibility Matrix

**Phase:** 2 (Hub)
**Current gap:** No version negotiation between `fmux`, `forged`, and
`forgehub`. As these components evolve independently (especially with multiple
edge nodes potentially running different versions), protocol drift is a real
risk.

**Recommendation:** Implement a simple compatibility check at connection time:

- Each binary embeds its version and minimum compatible protocol version.
- On connect, exchange versions. If incompatible, return a clear error:
  `"forged v0.3.0 requires fmux >= v0.2.0; you are running v0.1.5"`.
- The hub tracks the version of each connected edge. Dashboard shows edge
  versions so operators can see which nodes need updating.
- `forgehub check` warns if any connected edge is running an unsupported
  version.

Happy handles this with a version check on daemon connect that triggers
automatic restart. For forgemux, the reporting and clear error messages are
more appropriate (let operators manage upgrades explicitly).

**Effort delta:** +1-2 days in Phase 2.

---

## 11. Optimistic Concurrency on Session State

**Phase:** 3 (Browser Attach)
**Current gap:** The session store is file-based JSON with no concurrency
control. Once multiple clients (CLI, browser, Foreman) can interact with a
session, concurrent writes become possible.

**Recommendation:** Add a version field to `SessionRecord`. On write,
compare-and-swap the version number. If the version has changed since the
reader's last fetch, the write fails with a conflict error and the client
retries with fresh state.

This is a small change but prevents subtle bugs:
- Two browser clients both try to send input at the same time.
- The Foreman tries to inject a command while the engineer is typing.
- A policy change races with a state transition.

Happy uses this pattern for all session metadata and dynamic state updates. The
server rejects stale writes and returns current state for retry.

**Effort delta:** +1-2 days in Phase 3.

---

## 12. Ephemeral vs Persistent Event Classification

**Phase:** 2-3 (Hub and Browser Attach)
**Current gap:** The design treats all events uniformly. The ring buffer,
transcript writer, and WebSocket bridge do not distinguish between events that
must be replayed on reconnect and events that are only meaningful in real time.

**Recommendation:** Tag every event in the wire protocol as `durable` or
`ephemeral` from the first hub release:

- **Durable:** state transitions, transcript chunks, tool calls, usage records.
  Stored in the ring buffer, persisted to the transcript/event log, replayed on
  `RESUME`.
- **Ephemeral:** agent "thinking" indicator, live token counter, cursor
  position, active-user presence, typing indicator. Broadcast to connected
  clients but excluded from the ring buffer, transcript, and `RESUME` replay.

**Why this matters:**
- Replaying stale ephemeral events on reconnect is confusing (e.g., "user X is
  typing" from 10 minutes ago).
- High-frequency ephemeral events (thinking indicator toggling multiple times
  per second) would bloat the ring buffer and waste storage.
- The hub's optional store-and-forward buffer should not waste space on
  ephemeral data.

Happy makes this distinction explicitly in its sync model and it keeps storage
lean and reconnect semantics clean. It's a small design decision but much
easier to build in from the start than to retrofit after events are already
being stored uniformly.

**Effort delta:** Minimal if decided at wire protocol design time. Requires a
one-bit flag on the event envelope.

---

## 13. Artifact Store for Session Attachments

**Phase:** 4-5 (Dashboard and Foreman)
**Current gap:** There is no mechanism to attach structured outputs (Foreman
reports, git diffs, terminal snapshots, usage summaries) to a session in a way
that is discoverable via the hub API.

**Recommendation:** Add an artifact store keyed by session ID:

- Each artifact has metadata (type, timestamp, size) and a blob.
- Events in the session stream reference artifacts by ID rather than inlining
  content.
- The dashboard renders artifacts in a side panel (diff viewer, report viewer).
- For E2E encryption, artifacts are encrypted at the edge with the session key.

This avoids two antipatterns: inlining large structured outputs into the
transcript (noisy, hard to retrieve) and storing ad-hoc files in the session
directory (undiscoverable, no API).

Happy uses this pattern for rich attachments with per-record encryption keys.

**Effort delta:** +2-3 days in Phase 4 or 5. Low priority for MVP but useful
once the Foreman produces structured reports and the dashboard needs to display
them.

---

## 14. RPC Inspection Surface for Dashboard and Foreman

**Phase:** 4-5 (Dashboard and Foreman)
**Current gap:** Forgemux already has gRPC/HTTP endpoints for session lifecycle
and `fmux inject` for command injection. What it lacks is a structured
read-only inspection API for querying file contents, code search, and diffs
from the dashboard or Foreman without opening a full terminal.

**Recommendation:** Add a bounded set of read-only inspection RPCs on forged:

- `file.read(session_id, path)` -- read a file from the session's working
  directory.
- `file.list(session_id, glob)` -- list files matching a pattern.
- `git.diff(session_id)` -- current uncommitted changes.
- `git.log(session_id, n)` -- recent commits.

Gate access behind the same auth as terminal attach. Do not include write
operations -- `fmux inject` remains the only write path.

**Use cases:**
- Dashboard "inspect" panel shows files the agent has modified without terminal
  attach.
- Foreman queries file state directly instead of running `git diff` inside its
  own tmux session.
- Operators check agent progress without disrupting the session.

Happy exposes similar RPCs (bash, file read/write, ripgrep, difftastic) over
its WebSocket connection. For forgemux, the read-only subset is the right
scope -- it complements the terminal rather than replacing it.

**Effort delta:** +3-4 days in Phase 4 or 5.

---

## 15. CLI Diagnostics Command

**Phase:** 0-2
**Current gap:** `forged check` validates daemon config, certs, tmux, and agent
binaries. There is no equivalent on the CLI side. The CLI config location
(`~/.config/forgemux/config.toml`) and TLS cert paths are specified, but
CLI-side cached state (session history, pairing tokens, logs) is not.

**Recommendation:** Add `fmux doctor` that validates:

- Config file syntax and required fields.
- TLS certificate chain (CA cert valid, client cert not expired).
- Hub reachability (can connect, version compatible).
- Edge reachability for configured aliases.
- Local directory layout (cache dir, log dir exist and are writable).

Standardize CLI cache at `~/.cache/forgemux/` and logs at
`~/.local/state/forgemux/logs/` following XDG conventions. Document the full
filesystem layout for both CLI and daemon in the design doc.

**Effort delta:** +1-2 days. Most naturally fits in Phase 0 (basic check) with
enrichment as hub and edge connectivity are added in Phase 2.

---

## 16. QR Code Pairing for Browser Attach

**Phase:** 3 (Browser Attach)
**Current gap:** The design specifies JWT authentication for browser clients but
does not describe how a user obtains the JWT. API token management (Phase 7) is
the planned approach, but that is several phases away.

**Recommendation:** For Phase 3, add a QR code pairing flow as a lightweight
auth mechanism that does not require the full RBAC system:

1. Engineer runs `fmux pair` in a terminal.
2. CLI generates a short-lived pairing token and displays a QR code.
3. Engineer scans the QR with their phone's camera or opens the URL on their
   laptop browser.
4. The browser receives a session-scoped JWT that grants access to the specific
   session(s) the engineer owns.

This avoids the need to manage API tokens for casual browser access and is
familiar from Happy's QR-based pairing flow.

**Effort delta:** +3-4 days in Phase 3.

---

## Summary of proposed changes

| # | Recommendation | Target Phase | Effort Delta | Impact |
|---|---------------|-------------|-------------|--------|
| 1 | Agent log file watching for state detection | 0 | +1 week | High -- reduces WaitingInput false positives |
| 2 | Daemon lifecycle hardening | 0 | +2-3 days | Medium -- operational reliability |
| 3 | Structured event log alongside raw transcripts | 0-1 | +3-4 days | Medium -- enables Foreman, search, dashboard |
| 4 | Wire protocol versioning | 2-3 | Minimal | High -- prevents painful retrofit |
| 5 | Reconsider mobile access | 3-4 | +1 week | Medium -- extends existing work to mobile |
| 6 | E2E encryption for hub relay | 3 | +1-2 weeks | Medium -- security for shared infrastructure |
| 7 | Notification delivery reliability | 1 | +2-3 days | Medium -- critical path for remote UX |
| 8 | Agent adapter trait | 0 | +2-3 days | Medium -- consolidates per-agent logic |
| 9 | Session handoff workflow | 7 | +3-4 days | Low -- useful but not blocking |
| 10 | CLI-daemon version compatibility | 2 | +1-2 days | Medium -- prevents silent breakage |
| 11 | Optimistic concurrency on session state | 3 | +1-2 days | Low -- prevents rare concurrent bugs |
| 12 | Ephemeral vs persistent event classification | 2-3 | Minimal | High -- storage, replay, reconnect UX |
| 13 | Artifact store for session attachments | 4-5 | +2-3 days | Low -- enables Foreman reports, dashboard |
| 14 | RPC inspection surface | 4-5 | +3-4 days | Medium -- dashboard and Foreman file queries |
| 15 | CLI diagnostics command | 0-2 | +1-2 days | Low -- operational maturity |
| 16 | QR code pairing for browser attach | 3 | +3-4 days | Medium -- unblocks browser auth before RBAC |

**Total additional effort across all phases:** ~5-7 weeks, spread across the
existing roadmap. The highest-value items (#1, #4, #8, #12) are also the
cheapest to add early.

### Priority Tiers

**Do first (Phase 0 additions):**
- #1 Agent log file watching -- directly improves the MVP's core differentiator.
- #2 Daemon lifecycle hardening -- cheap operational hygiene.
- #8 Agent adapter trait -- foundational abstraction that simplifies later work.
- #15 CLI diagnostics -- basic `fmux doctor` for config and connectivity checks.

**Do at phase design time (low cost, high regret if missed):**
- #4 Wire protocol versioning -- design-time decision, expensive to retrofit.
- #10 CLI-daemon version compatibility -- trivial to add at Phase 2, painful to
  add later with deployed nodes.
- #12 Ephemeral vs persistent events -- one-bit flag on the event envelope,
  affects ring buffer, transcript, and replay semantics. Must be decided before
  the wire protocol solidifies.

**Do when the phase is in flight:**
- #3 Structured event log -- start simple in Phase 0, enrich over time.
- #7 Notification reliability -- part of Phase 1 implementation.
- #5 Mobile access -- incremental addition to Phase 3-4.
- #11 Optimistic concurrency -- part of Phase 3 session store work.
- #16 QR pairing -- part of Phase 3 auth story.
- #14 RPC inspection surface -- part of Phase 4 dashboard or Phase 5 Foreman.

**Defer until dependency phases land:**
- #6 E2E encryption -- optional addition to Phase 3, can ship after initial
  browser attach.
- #9 Session handoff -- depends on user identity (Phase 7).
- #13 Artifact store -- useful once Foreman (Phase 5) produces structured
  reports and dashboard (Phase 4) needs to display them.

---

## Execution Plan (Current Codebase)

This section translates the recommendations above into a concrete, staged plan
for the current forgemux implementation (see CHANGELOG for recent work). The
goal is to integrate high-value items early without blocking ongoing feature
delivery.

### Phase 0 Extensions (MVP Stability)

**Objective:** Improve state detection accuracy and daemon robustness without
changing user-facing workflows.

Planned work:
- Implement agent log watching for `WaitingInput` detection (Recommendation #1).
- Introduce an `AgentAdapter` trait and move agent-specific logic behind it (#8).
- Add daemon lifecycle hardening: PID lock, orphan cleanup, and explicit state
  persistence boundaries (#2).
- Add `fmux doctor` as a CLI-side diagnostics command (#15).

Execution steps:
1. Add `agent_adapter` module with Claude adapter scaffolding and existing
   prompt regex logic.
2. Add file watcher infrastructure (inotify/kqueue) behind a small trait for
   testability.
3. Feed agent log signals into state detection with clear precedence over PTY
   heuristics.
4. Add PID lock + orphan cleanup on `forged run` startup; document behaviour.
5. Implement `fmux doctor` with config + TLS + hub reachability checks.

Exit criteria:
- `WaitingInput` accuracy improves with structured logs enabled.
- CLI shows clear diagnostics when config or connectivity is broken.
- `forged` refuses double-start and cleans up stale tmux sessions.

### Phase 1 Extensions (Notifications)

**Objective:** Add reliability and observability to notification delivery.

Planned work:
- Implement retry/backoff and fallback channel logic (#7).
- Write notification delivery events into the session event log (#3).

Execution steps:
1. Add notification delivery result records to the structured event log.
2. Implement a retry policy for webhook failures.
3. Add per-session rate limiting to avoid spam on flapping state changes.

Exit criteria:
- Webhook failures trigger retries and are visible in `fmux status`.
- Notification delivery is observable in the event log.

### Phase 2 Design-Time Decisions (Hub Protocol)

**Objective:** Lock down protocol invariants before the hub APIs harden.

Planned work:
- Define protocol versioning (#4) and compatibility checks (#10).
- Introduce durable vs ephemeral event classification (#12).

Execution steps:
1. Extend the protocol envelope with `protocol_version` and `durable` fields.
2. Add compatibility checks in `fmux` ↔ `forged` ↔ `forgehub` handshakes.
3. Update docs/specs to reflect durability rules and replay semantics.

Exit criteria:
- Protocol changes are versioned and compatibility is enforced.
- Ephemeral events no longer bloat ring buffers or snapshots.

### Phase 3 Extensions (Browser and Mobile Attach)

**Objective:** Make the attach experience robust and mobile-friendly.

Planned work:
- Add QR-based pairing flow for browser attach (#16).
- Ensure responsive dashboard layout for mobile watch mode (#5).
- Add optimistic concurrency on session state updates (#11).
- Optional: add E2E encryption as a follow-up, gated by an opt-in flag (#6).

Execution steps:
1. Implement QR pairing in CLI + hub; short-lived tokens yield session-scoped JWT.
2. Add responsive dashboard layout and test watch-mode on mobile Safari/Chrome.
3. Add session record versioning to reject stale writes.
4. Define encryption key exchange and make relay encryption opt-in.

Exit criteria:
- Browser attach works without API tokens.
- Mobile watch-mode is usable and reliable under poor connectivity.
- Conflicting writes are detected and retried safely.

### Phase 4–5 Extensions (Dashboard + Foreman)

**Objective:** Enable richer inspection and reporting without disrupting sessions.

Planned work:
- Introduce structured event logs as a first-class asset (#3).
- Add RPC inspection surface: read-only file/list/diff/log (#14).
- Add artifact store for foreman reports and diffs (#13).

Execution steps:
1. Standardize the event log schema and begin writing it on every session.
2. Add read-only inspection endpoints in `forged` and surface in dashboard.
3. Add artifact metadata and storage layout, encrypted when enabled.

Exit criteria:
- Foreman can fetch diffs and state without attaching to a terminal.
- Dashboard can render structured artifacts and event history.

### Phase 7 Extensions (Access Control)

**Objective:** Support session handoff with clear auditability.

Planned work:
- Implement session transfer workflow (#9).

Execution steps:
1. Add `fmux transfer` command and hub API.
2. Emit a transfer event with optional annotation.
3. Update dashboard to show ownership and transfer history.

Exit criteria:
- Sessions can be handed off without losing context.
- Ownership changes are tracked and visible in audit logs.

---

## Near-Term Next Steps (Concrete)

1. Implement the `AgentAdapter` trait and agent log watcher (Phase 0 extension).
2. Add PID lock + orphan cleanup in `forged` and document it.
3. Add `fmux doctor` and wire it into the CLI help.
4. Add notification retries and delivery logs (Phase 1 extension).
5. Update specs to include protocol versioning and durable/ephemeral events.

These steps align with the existing roadmap while reducing risk in later phases.
