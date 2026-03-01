# Forgemux Hub Dashboard — Implementation Plan

**Date:** 2026-02-27
**References:** `tech_design_spec.md`, `forgemux-hub-stories.md`, `forgemux-hub.jsx`

---

## Current State Summary

What exists today and can be built upon:

| Component | File | What exists |
|-----------|------|-------------|
| Core types | `forgemux-core/src/lib.rs` | `SessionRecord`, `SessionState`, `SessionId`, `SessionStore`, `StateDetector`, `AgentType`, `SessionRole` |
| Hub server | `forgehub/src/lib.rs` | `HubService` with edge registry, pairing/token auth, session aggregation from edges |
| Hub routes | `forgehub/src/main.rs` | REST (`/sessions`, `/sessions/:id/*`, `/edges/*`, `/pairing/*`) + WS (`/sessions/ws`, `/sessions/:id/attach`) |
| Edge server | `forged/src/server.rs` | REST (`/sessions/*`) + WS (`/sessions/:id/attach`) with stream protocol |
| Edge service | `forged/src/lib.rs` | `SessionService` with tmux integration, state detection, notifications, usage tracking |
| Stream protocol | `forged/src/stream.rs` | `StreamManager`, `EventRing`, `InputDeduper`, RESUME/EVENT/INPUT/ACK/SNAPSHOT |
| Dashboard | `dashboard/index.html` | Vanilla JS, light theme, session list via WS, attach via WS, start form, log/usage detail |

What does **not** exist yet:

- No `sqlx` or any database in forgehub (config is file-based)
- No `Organization`, `Workspace`, or `WorkspaceRepo` types
- No `Decision` type or decision queue
- No risk scoring or attention budget
- No replay events or structured timeline
- No Preact/HTM frontend (current dashboard is vanilla JS in a single HTML file)

---

## Cross-Cutting Concerns

These concerns span multiple phases and must be addressed consistently rather than bolted on piecemeal. Each is referenced in the relevant phase below.

### CC-1: forgemux-core Module Splitting

**Problem:** Phases 1-4 and 7 all add types to `forgemux-core/src/lib.rs`, which already has 580 lines. Adding Workspace, Decision, SessionHubMeta, RiskLevel, and ReplayEvent to a single file will make it unwieldy.

**Resolution:** Split `forgemux-core/src/lib.rs` into modules during Phase 1 before adding new types:

```
crates/forgemux-core/src/
├── lib.rs              # re-exports only
├── session.rs          # SessionRecord, SessionState, SessionId, SessionStore, SessionManager
├── state.rs            # StateDetector, StateSignal
├── workspace.rs        # Organization, Workspace, WorkspaceRepo, AttentionBudget  (Phase 1)
├── decision.rs         # Decision, DecisionContext, Severity, DecisionAction      (Phase 2)
├── meta.rs             # SessionHubMeta, RiskLevel, TestsStatus                   (Phase 3)
└── replay.rs           # ReplayEvent, ReplayEventType                             (Phase 7)
```

`lib.rs` becomes `pub mod session; pub mod state; ...` with `pub use` re-exports so downstream crates are unaffected.

**Phase:** Do the split as the first task of Phase 1, before adding new types.

### CC-2: Edge Heartbeat + `Unreachable` State

**Problem (from tech_design_review.md):** If an edge disconnects or crashes, the hub's session state must reflect this immediately. The current plan mentions "mark as stale after N missed polls" in Phase 8 but doesn't define the state or UX.

**Resolution:**

1. Add `Unreachable` to `SessionState` enum in `forgemux-core`. This is distinct from `Errored` (which means the agent process failed) — `Unreachable` means the edge is not responding.

2. In `poll_edges()` (Phase 3), track `last_seen` per edge. If 2 consecutive polls (6 seconds) fail, mark all sessions on that edge as `Unreachable`.

3. In the dashboard, render `Unreachable` sessions greyed out with a disconnected icon, distinct from `Errored` (red) and `Blocked` (amber).

4. When the edge comes back, sessions revert to their actual state on the next successful poll.

**Phase:** Define the state in Phase 1 (module split). Implement the detection in Phase 3. Render in Phase 5.

### CC-3: Edge-to-Hub Authentication

**Problem:** Edges POST decisions to the hub's `/decisions` endpoint, but the auth mechanism for this path isn't specified. The existing edge registration uses `/edges/register` + heartbeat, but decision forwarding needs auth too.

**Resolution:** Edges authenticate to hub using the same Bearer token mechanism. When an edge registers (`POST /edges/register`), the hub issues an edge-scoped token (or the edge uses its configured `api_token`). All edge-to-hub requests (decision forwarding, heartbeat, session cache updates) include this token. Hub validates it and scopes the request to the edge's registered identity.

**Phase:** Already partially implemented (edges have `api_tokens` config). Wire it into the decision forward path in Phase 4.

### CC-4: `goal` Field Provenance

**Problem:** `SessionHubMeta.goal` is displayed everywhere (fleet dashboard, decision cards, replay sidebar) but `SessionRecord` doesn't have a `goal` field and there's no spec for where it comes from.

**Resolution:** Add `goal: Option<String>` to `SessionRecord` in Phase 1. The goal is set at session start time via:
1. Explicit `--goal "Add Stripe webhook verification"` flag on `fmux start` / `POST /sessions`
2. If not provided, derived from the first user message in the agent's JSONL log (if parseable)
3. Falls back to `"(no goal set)"` in the dashboard

The hub stores the goal in `session_cache.hub_meta_json` alongside workspace_id and other enrichment data.

**Phase:** Add the field in Phase 1. Populate it in Phase 3 (session enrichment).

### CC-5: Replay Pagination

**Problem (from tech_design_review.md):** Fetching full timeline and logs for a long-running session (thousands of events) via a single REST call will choke the client.

**Resolution:**

1. `/sessions/:id/replay/timeline` supports cursor-based pagination: `?after=<event_id>&limit=100`. Default limit 100 events.
2. The Timeline sidebar initially loads only high-level events (System, Decision, Switch, Test). Tool-level events (Read, Edit) are lazy-loaded when the user scrolls or clicks "show all."
3. `/sessions/:id/replay/terminal` supports `?offset=<byte>&limit=<bytes>` for chunked log fetching.

**Phase:** Define pagination params in Phase 7 (replay endpoints). Frontend implements lazy loading in Phase 7 (replay UI).

### CC-6: Font Loading Strategy

**Problem (from tech_design_review.md):** Three Google Fonts (Poppins, Outfit, JetBrains Mono) risk FOUT (Flash of Unstyled Text).

**Resolution:** In `dashboard/index.html`:
1. Use `<link rel="preload" as="font">` for the most critical weights (Poppins 400, Outfit 300, JetBrains Mono 400)
2. Set `font-display: swap` on all `@font-face` declarations
3. Define CSS fallback stacks: `'Poppins', system-ui, sans-serif`, `'Outfit', system-ui, sans-serif`, `'JetBrains Mono', ui-monospace, monospace`

**Phase:** Phase 5 (SPA foundation, `index.html` setup).

### CC-7: Database Migration Story

**Problem:** Phase 1 uses `CREATE TABLE IF NOT EXISTS` which works for initial deployment but has no plan for schema changes after production data exists.

**Resolution:** Use `sqlx`'s built-in migration system:
1. Create `crates/forgehub/migrations/` directory
2. Initial migration: `001_init.sql` with all `CREATE TABLE` statements
3. Subsequent phases add numbered migrations: `002_add_replay_events.sql`, etc. (currently all schema lives in `001_init.sql`; follow-up migrations are not yet split out)
4. `sqlx::migrate!()` runs pending migrations on hub startup
5. Migrations are forward-only (no down migrations for v1)

**Phase:** Set up the migration directory in Phase 1. Each subsequent phase adds its own migration file.

### CC-8: Observability for New Code Paths

**Problem:** The existing crates use `tracing` but the plan doesn't specify structured spans/events for the new decision lifecycle, risk scoring, and edge polling.

**Resolution:** Add `tracing` instrumentation:
- `#[tracing::instrument]` on all `HubService` public methods
- Named spans: `decision.create`, `decision.resolve`, `risk.compute`, `edge.poll`, `edge.poll.{edge_id}`
- Key events: `tracing::info!(decision_id, action, "decision resolved")`, `tracing::warn!(edge_id, "edge poll failed")`
- Dashboard WS connection lifecycle: `tracing::debug!(client_id, "ws connected")` / `"ws disconnected"`

This is not a separate phase — it's a coding practice applied in every phase. The test plan should verify that resolved decisions emit trace events (check via `tracing_test` or `tracing-subscriber`'s test layer).

### CC-9: Competitive Ideas Integration

The competitive analysis (`comp-report.md`) identified 8 high-leverage ideas to incorporate. Rather than listing them separately, they are wired into phases below:

| Competitive Idea | Integrated Into |
|-------------------|----------------|
| Credential scrubbing | Phase 4 (edge log pipeline), Phase 7 (replay) — scrub before persistence |
| Worktree lifecycle | Deferred (separate follow-up; existing `--worktree` flag is a start) |
| Session templates | Phase 1 (workspace config TOML can include template definitions) |
| Context health signal | Phase 3 (`SessionHubMeta.context_pct` + risk scoring) — already included |
| Outbound webhooks | Deferred to post-v1 (Phase 8 notes it as future) |
| Agent event protocol | Phase 7 (replay JSONL schema is the first step toward structured events) |
| Session fork/resume | Deferred (requires `agent_session_id` in `SessionRecord`, add field in Phase 1) |
| Agent CLI plugin system | Deferred (existing `AgentConfig` per agent in forged is sufficient for v1) |

Items marked "deferred" are tracked but not blocking the dashboard implementation. The field additions (`goal`, `agent_session_id`) are cheap to add in Phase 1 to preserve future optionality.

### CC-10: Atomic Merge Distributed Transaction Warning

**From tech_design_review.md:** The Atomic Merge feature (creating N synchronized PRs across N repos) is a distributed transaction problem. If PR #1 succeeds but PR #2 fails, the system enters an inconsistent state.

**Resolution:** Atomic Merge is deferred to v2 (button disabled in the UI). When implemented, it must use a Saga pattern or explicit state machine:
1. Create all PRs as draft
2. Run checks on all
3. Only merge all if all checks pass
4. UI must handle partial failure: "2/3 PRs created, retry the 3rd?"

This is noted here as an architectural constraint for the v2 implementer. The v1 UI shows the button disabled with a tooltip explaining it's coming.

---

## Phase 1: Hub Storage Foundation + Workspace Model

**Goal:** Add SQLite persistence to forgehub, split forgemux-core into modules, introduce the Organization/Workspace/Repo hierarchy, and add future-proofing fields.

**Cross-cutting concerns addressed:** CC-1 (module split), CC-2 (Unreachable state), CC-4 (goal field), CC-7 (migration setup), CC-9 (session templates, fork/resume fields).

### 1.0 Split forgemux-core into modules (CC-1)

**Before adding any new types,** split `forgemux-core/src/lib.rs` into modules:

- Move `SessionRecord`, `SessionState`, `SessionId`, `SessionStore`, `SessionManager`, `sort_sessions`, `state_priority` into `src/session.rs`
- Move `StateDetector`, `StateSignal` into `src/state.rs`
- Move `RepoRoot` into `src/repo.rs`
- `lib.rs` becomes re-exports: `pub mod session; pub mod state; pub mod repo; pub use session::*; pub use state::*; pub use repo::*;`
- Add `Unreachable` variant to `SessionState` (CC-2). Update `state_priority()` to sort it between Errored and Terminated.

**Verification:** `cargo test --workspace` passes with zero changes to downstream crates.

### 1.1 Add sqlx + chrono-tz dependencies

**File:** `crates/forgehub/Cargo.toml`

- Add `sqlx = { version = "0.7", features = ["runtime-tokio", "sqlite", "postgres", "chrono"] }`
- Add `chrono-tz = "0.9"`

**Verification:** `cargo check -p forgehub` compiles.

### 1.2 Define Organization, Workspace, WorkspaceRepo types

**File:** `crates/forgemux-core/src/workspace.rs` (new module)

Add the following types (all `Serialize + Deserialize + Clone`):

```rust
pub struct Organization { pub id: String, pub name: String }
pub struct WorkspaceRepo { pub id: String, pub label: String, pub icon: String, pub color: String }
pub struct AttentionBudget { pub used: u32, pub total: u32, pub reset_tz: String }
pub struct Workspace {
    pub id: String, pub org_id: String, pub name: String,
    pub repos: Vec<WorkspaceRepo>, pub members: Vec<String>,
    pub attention_budget: AttentionBudget,
}
```

**Also in this step (CC-4, CC-9):** Add future-proofing fields to `SessionRecord` in `src/session.rs`:
- `pub goal: Option<String>` — set at session start via `--goal` flag or derived from first agent message
- `pub agent_session_id: Option<String>` — for session fork/resume (stores Claude conversation ID)

**Verification:** Unit tests for serde roundtrip. Existing tests still pass with new Optional fields (backward-compatible deserialization).

### 1.3 Create SQL schema and DB init (CC-7)

**File:** `crates/forgehub/migrations/001_init.sql` (new)

Contains all `CREATE TABLE` statements from tech_design_spec.md section 3.3.

**File:** `crates/forgehub/src/db.rs` (new)

- Create `init_db(data_dir: &Path) -> Result<SqlitePool>`
- Uses `sqlx::migrate!()` to run pending migrations from `migrations/` directory
- Schema exactly as specified in `tech_design_spec.md` section 3.3

**File:** `crates/forgehub/src/lib.rs`

- Add `db: sqlx::SqlitePool` field to `HubService`
- Call `init_db()` in `HubService::new()`

**Verification:** Hub starts, creates `hub.db` in data dir, tables exist.

### 1.4 Add workspace REST endpoints

**File:** `crates/forgehub/src/lib.rs`

- `HubService::list_workspaces(&self) -> Vec<Workspace>`
- `HubService::get_workspace(&self, id: &str) -> Option<Workspace>`
- For v1, seed workspace from a config file or environment variable. Full CRUD is deferred.

**File:** `crates/forgehub/src/main.rs`

- Register `GET /workspaces` and `GET /workspaces/:id` routes
- Wire to `HubService` methods
- Workspace response includes computed `attention_budget.used` from `attention_budget_log`

**Verification:** `curl /workspaces` returns seeded workspace. `curl /workspaces/:id` returns detail with budget.

---

## Phase 2: Decision Queue Backend

**Goal:** Implement the full decision data model, REST API, and WebSocket broadcast.

### 2.1 Define Decision types in forgemux-core

**File:** `crates/forgemux-core/src/decision.rs` (new module, per CC-1)

Add all decision-related types:

```rust
pub enum DecisionContext { Diff { file, lines }, Log { text }, Screenshot { description } }
pub struct DiffLine { pub line_type: DiffLineType, pub text: String }
pub enum DiffLineType { Ctx, Add, Del }
pub enum Severity { Critical = 0, High = 1, Medium = 2, Low = 3 }
pub struct Decision { /* all fields from tech_design_spec.md section 3.2 */ }
pub struct DecisionResolution { pub action, pub reviewer, pub comment, pub resolved_at }
pub enum DecisionAction { Approve, Deny, Comment }
```

**Verification:** Serde roundtrip tests. Severity ordering test (`Critical < High < Medium < Low`).

### 2.2 Implement decision CRUD in hub service

**File:** `crates/forgehub/src/db.rs`

- `insert_decision(pool, decision) -> Result<()>`
- `get_decision(pool, id) -> Result<Decision>`
- `list_decisions(pool, workspace_id, repo_id?, status?) -> Result<Vec<Decision>>`
- `resolve_decision(pool, id, resolution) -> Result<()>`
- `log_budget_action(pool, workspace_id, decision_id, reviewer, action) -> Result<()>`
- `budget_used_today(pool, workspace_id, timezone) -> Result<u32>`

**File:** `crates/forgehub/src/lib.rs`

- `HubService::create_decision(&self, decision) -> Result<Decision>`
  - Assigns sequential ID (`D-XXXX`)
  - Inserts into DB
  - Broadcasts `DecisionEvent::Created` via `decision_tx`
- `HubService::resolve_decision(&self, id, action, reviewer, comment) -> Result<()>`
  - Updates DB
  - Logs to `attention_budget_log`
  - Forwards to edge: `POST /sessions/:session_id/decision-response`
  - Broadcasts `DecisionEvent::Resolved`

**Verification:** Integration test: create decision, list it, resolve it, verify budget log incremented.

### 2.3 Add decision REST endpoints

**File:** `crates/forgehub/src/main.rs`

Register routes:

```
GET    /decisions?workspace_id=&repo_id=&status=
GET    /decisions/:id
POST   /decisions/:id/approve   { reviewer, comment? }
POST   /decisions/:id/deny      { reviewer, comment? }
POST   /decisions/:id/comment   { reviewer, comment }
```

Each action handler:
1. Calls `resolve_decision()`
2. Returns `{ decision_id, action, session_unblocked: bool }`

**Verification:** Full REST roundtrip via curl/httpie.

### 2.4 Add decisions WebSocket channel

**File:** `crates/forgehub/src/main.rs`

- Add `decision_tx: broadcast::Sender<DecisionEvent>` to `HubService`
- Register `GET /decisions/ws?workspace_id=`
- On connect: send current pending decisions as initial payload
- On `DecisionEvent::Created`: push `{ "type": "decision_created", "decision": {...} }`
- On `DecisionEvent::Resolved`: push `{ "type": "decision_resolved", "decision_id": "...", "action": "..." }`

**Verification:** Open WS, create decision via REST, see WS event. Resolve via REST, see WS event.

---

## Phase 3: Session Enrichment + Risk Scoring

**Goal:** Extend session data with hub metadata (goal, risk, repos, context %, tests, cost) and implement risk scoring.

**Cross-cutting concerns addressed:** CC-2 (edge heartbeat + Unreachable), CC-4 (goal population), CC-8 (tracing for edge polling and risk).

### 3.1 Define SessionHubMeta and related types

**File:** `crates/forgemux-core/src/meta.rs` (new module, per CC-1)

```rust
pub struct SessionHubMeta {
    pub workspace_id: String, pub goal: String, pub risk: RiskLevel,
    pub context_pct: u8, pub touched_repos: Vec<String>,
    pub pending_decisions: u32, pub tests_status: TestsStatus,
    pub tokens_total: String, pub estimated_cost_usd: f64,
    pub lines_added: u32, pub lines_removed: u32, pub commits: u32,
}
pub enum RiskLevel { Green, Yellow, Red }
pub enum TestsStatus { Passing, Failing, Pending, None }
```

**Verification:** Serde tests.

### 3.2 Implement risk scoring on hub

**File:** `crates/forgehub/src/risk.rs` (new)

```rust
pub fn compute_risk(meta: &SessionHubMeta, pending: &[Decision]) -> RiskLevel
```

Logic (from tech_design_spec.md section 5.4):
- Red: context > 85% OR errored OR oldest pending decision > 15m
- Yellow: context >= 70% OR tests failing OR pending decision > 5m
- Green: everything else

**Verification:** Unit tests for each threshold boundary.

### 3.3 Implement edge polling loop with session cache (CC-2, CC-8)

**File:** `crates/forgehub/src/lib.rs`

- Add background tokio task: `poll_edges(hub: Arc<HubService>)`
- Every 3 seconds, for each registered edge:
  - `GET /sessions` from edge
  - Merge into `session_cache` table via `db.rs`
  - Populate `goal` from `SessionRecord.goal` or derive from first agent message (CC-4)
  - Compute risk scores (requires pending decisions from DB)
  - If anything changed, broadcast `SessionEvent::Updated` via `session_tx`
  - Recompute workspace stats, broadcast `SessionEvent::StatsChanged` if changed
  - Recompute attention budget, broadcast `SessionEvent::BudgetChanged` if changed

**Edge heartbeat and Unreachable detection (CC-2):**
- Track `consecutive_failures: u8` per edge in memory
- On successful poll: reset to 0, update `last_seen`
- On failed poll: increment. If `consecutive_failures >= 2` (6 seconds):
  - Mark all sessions on that edge as `Unreachable` in session cache
  - Emit `tracing::warn!(edge_id, failures = consecutive_failures, "edge unreachable")`
  - Broadcast `SessionEvent::Updated` for affected sessions
- On recovery (next successful poll after failures): revert sessions to actual state, log recovery

**File:** `crates/forgehub/src/db.rs`

- `upsert_session_cache(pool, session_id, workspace_id, edge_id, meta_json, state) -> Result<()>`
- `list_cached_sessions(pool, workspace_id) -> Result<Vec<CachedSession>>`
- `mark_edge_sessions_unreachable(pool, edge_id) -> Result<u32>` (returns count of affected sessions)

### 3.4 Extend sessions WebSocket payload

**File:** `crates/forgehub/src/main.rs`

Currently `/sessions/ws` pushes a flat list of `SessionRecord` every 2 seconds. Change to:

- Push structured messages:
  - `{ "type": "session_update", "session": { /* SessionRecord + hub_meta */ } }`
  - `{ "type": "stats_update", "stats": { active, blocked, queued, complete, cost_today_usd } }`
  - `{ "type": "budget_update", "budget": { used, total } }`
- Push on change (driven by `session_tx` broadcast), with 3s heartbeat keepalive

**Breaking change:** The current dashboard JS parses the WS payload as a raw session array. The new dashboard will use the structured format. Until the new dashboard is live, maintain backward compatibility by sending the flat array as a `sessions_list` message type alongside the new events, or gate on a query param (`?v=2`).

### 3.5 Extend GET /sessions response

**File:** `crates/forgehub/src/main.rs`

Change `GET /sessions?workspace_id=` response to include:

```json
{
  "sessions": [ { /* SessionRecord fields */, "hub_meta": { /* SessionHubMeta */ } } ],
  "stats": { "active": 3, "blocked": 1, "queued": 2, "complete": 1, "cost_today_usd": 5.83 }
}
```

**Verification:** `curl /sessions?workspace_id=ws-checkout` returns enriched sessions with risk levels.

---

## Phase 4: Edge Decision Flow

**Goal:** Enable agents to emit decision requests and receive responses through the edge.

**Cross-cutting concerns addressed:** CC-3 (edge-to-hub auth), CC-9 (credential scrubbing).

### 4.0 Credential scrubbing pipeline (CC-9)

**File:** `crates/forgemux-core/src/scrub.rs` (new module)

Before any log content or decision context is forwarded from edge to hub or persisted to disk, pass it through a scrubbing pipeline. Implement a `scrub(text: &str) -> String` function using regex patterns for:
- AWS keys (`AKIA[0-9A-Z]{16}`)
- Generic API keys/tokens (long hex/base64 strings after `key=`, `token=`, `secret=`, `password=`)
- Private keys (`-----BEGIN .* PRIVATE KEY-----`)
- Connection strings (`://[^:]+:[^@]+@`)

Apply `scrub()` in:
- `POST /sessions/:id/decision` (context field, before forwarding to hub)
- `emit_replay_event()` (action/result fields, before writing to JSONL)
- Decision context rendered in dashboard (defense in depth, server-side scrub is primary)

### 4.1 Add decision endpoints to forged (CC-3)

**File:** `crates/forged/src/server.rs`

Register two new routes:

```
POST /sessions/:id/decision           # Agent/sidecar emits a decision request
POST /sessions/:id/decision-response  # Hub forwards reviewer's action
```

**`POST /sessions/:id/decision`:**
- Accepts `CreateDecisionPayload { question, context, severity, tags, impact_repo_ids }`
- Runs `scrub()` on context before forwarding (CC-9)
- Enriches with session metadata (agent_goal, repo, workspace)
- Forwards to hub: `POST /decisions` on forgehub, using edge's Bearer token (CC-3)

**`POST /sessions/:id/decision-response`:**
- Accepts `{ action, reviewer, comment }`
- Formats response text (e.g., `"Approved: proceed with env vars approach"`)
- Injects into tmux session via `SessionService::send_keys()`

**Verification:** Manual test: POST decision to forged, verify it appears in hub's `/decisions`. Resolve via hub, verify text injected into tmux.

### 4.2 Add Claude Code hook for decision detection

**Documentation / config template:** Create a hook configuration example that can be placed in the agent's settings:

```json
{
  "hooks": {
    "notification": [{
      "matcher": { "type": "tool_use", "tool": "AskHumanQuestion" },
      "command": "curl -s -X POST http://localhost:${FORGED_PORT}/sessions/${SESSION_ID}/decision -d @-"
    }]
  }
}
```

**File:** `crates/forged/src/lib.rs`

- When starting a session, set environment variables `FORGED_PORT` and `SESSION_ID` in the tmux environment so hooks can reference them.

### 4.3 Extend StateDetector for decision pattern detection (sidecar fallback)

**File:** `crates/forgemux-core/src/lib.rs`

- Add decision-like prompt patterns to `StateDetector`: lines ending with `?` after multi-line context, `[Y/n]`, `approve/deny` patterns
- When detected AND state transitions to `WaitingInput`, forged can optionally auto-create a decision with surrounding context from the PTY ring buffer

**File:** `crates/forged/src/lib.rs`

- In `refresh_states()`, when a session transitions to `WaitingInput` and a decision-like pattern is detected:
  - Capture last N lines from ring buffer as context
  - POST to hub as a decision with `severity: medium` (conservative default)

**Verification:** Integration test: agent prints a question pattern, forged detects it, decision appears in hub.

---

## Phase 5: Frontend — Preact SPA Foundation + Fleet Dashboard

**Goal:** Replace the current vanilla JS dashboard with the Preact+HTM SPA, starting with the fleet dashboard view.

**Cross-cutting concerns addressed:** CC-2 (Unreachable rendering), CC-6 (font loading).

### 5.1 Set up dashboard directory structure

**Directory:** `dashboard/`

Create the new SPA file structure alongside the existing `index.html` (rename existing to `index-legacy.html` for fallback):

```
dashboard/
├── index.html              # new SPA shell
├── app.js                  # main entry, router, top nav
├── lib/
│   ├── preact.module.js    # vendored preact (~3KB)
│   ├── htm.module.js       # vendored htm
│   └── hooks.module.js     # vendored preact/hooks
├── components/
│   ├── shared.js           # Dot, Badge, RepoPill, MiniBar, Card, SectionLabel
│   ├── fleet.js            # FleetDashboard
│   ├── decisions.js        # (placeholder)
│   ├── replay.js           # (placeholder)
│   └── nav.js              # TopNav
├── services/
│   ├── api.js              # REST client
│   └── ws.js               # WebSocket manager with auto-reconnect
├── theme.js                # T token object
└── state.js                # shared state (context or signals)
```

**File:** `dashboard/index.html`

- Minimal shell: loads Google Fonts (Poppins, Outfit, JetBrains Mono), sets dark bg, loads `app.js` as ES module
- No build step — all `<script type="module">` imports
- **Font loading strategy (CC-6):** `<link rel="preload" as="font">` for critical weights (Poppins 400, Outfit 300, JetBrains Mono 400). All `@font-face` use `font-display: swap`. CSS fallback stacks: `'Poppins', system-ui, sans-serif` / `'Outfit', system-ui, sans-serif` / `'JetBrains Mono', ui-monospace, monospace`

**File:** `dashboard/lib/` — vendor Preact, HTM, hooks modules (download from CDN, save locally)

### 5.2 Port theme tokens and shared components

**File:** `dashboard/theme.js`

- Export the `T` object exactly as defined in `forgemux-hub.jsx` and `tech_design_spec.md` section 8.3

**File:** `dashboard/components/shared.js`

Port from `forgemux-hub.jsx`:
- `Dot({ color, size, pulse })`
- `Badge({ children, color, bg })`
- `RepoPill({ repoId, repos })`
- `MiniBar({ value, max, color, h })`
- `SectionLabel({ children })`
- `Card({ children, style })`
- Helper functions: `riskColor()`, `statusColor()`, `contextColor()`, `severityColor()`
- `statusColor()` must handle `unreachable` state (CC-2): render greyed out (`T.t4`) with a disconnected icon

Convert from JSX to HTM tagged template syntax:
```js
// JSX:  <span style={{color: T.t1}}>text</span>
// HTM:  html`<span style=${{color: T.t1}}>text</span>`
```

**Verification:** Each component renders correctly in isolation.

### 5.3 Build TopNav component

**File:** `dashboard/components/nav.js`

Port from `forgemux-hub.jsx` `ForgemuxHub` shell:
- Logo (forgemux branding with gradient icon)
- Org / Workspace breadcrumb
- Nav tabs: Dashboard, Decisions (with pending count badge), Session Replay
- Repo indicators strip
- Live connection indicator (`Dot` + "live" / "reconnecting" / "disconnected")

State:
- `currentView` (fleet | queue | replay)
- `connectionStatus` (from WS manager)

### 5.4 Build WebSocket manager and API client

**File:** `dashboard/services/ws.js`

- `connectWS(path, { onMessage, onStatus })` — as specified in tech_design_spec.md section 8.7
- Auto-reconnect with 3-second delay
- Drives live indicator state

**File:** `dashboard/services/api.js`

- `fetchJSON(path, opts)` with Bearer token from `localStorage`
- `api` object with all endpoint methods as specified in tech_design_spec.md section 8.8

### 5.5 Build FleetDashboard view

**File:** `dashboard/components/fleet.js`

Port from `forgemux-hub.jsx` `FleetDashboard`:
- `AttentionBudgetMeter` — conic-gradient ring, remaining count, reset note
- `StatCard` — five metric cards (Active, Blocked, Queued, Complete, Cost Today)
- `SessionRow` — risk-bordered card with: risk dot (pulsing for red), goal, status badge, pending decisions badge, repo pills, context health bar + %, test badge, token/cost/uptime stats
- `QueuedPanel` — compact rows with goal, repo pills, model
- `CompletedPanel` — compact rows with goal, repo pills, lines +/-, cost, duration

**Data wiring:**
- On mount: `api.sessions(workspaceId)` for initial load
- Subscribe to `/sessions/ws?workspace_id=` for real-time updates
- On `session_update`: merge into state, re-sort by risk
- On `stats_update`: update stat cards
- On `budget_update`: update meter

### 5.6 Wire SPA routing and main entry

**File:** `dashboard/app.js`

- Import all view components
- Simple hash-based routing (`#fleet`, `#queue`, `#replay`)
- Render `TopNav` + active view
- Initialize WS connections on mount
- Pass workspace context down

**File:** `crates/forgehub/src/main.rs`

- Update static file serving to serve from new `dashboard/` structure
- Keep `/` serving `dashboard/index.html`

**Verification:** Dashboard loads in browser, shows fleet view with real session data from hub, live indicator works, session cards render with correct risk colors.

---

## Phase 6: Frontend — Decision Queue

**Goal:** Build the full decision queue UI with filtering, expandable context, and quick actions.

**Cross-cutting concerns addressed:** CC-10 (Atomic Merge button disabled with tooltip).

### 6.1 Build DecisionQueue view

**File:** `dashboard/components/decisions.js`

Port from `forgemux-hub.jsx` `DecisionQueue`:

**RepoFilterBar:**
- "All repos" button + per-repo buttons from workspace config
- Client-side filter on `repo_id`
- Active filter uses repo color highlight

**DecisionCard (collapsed):**
- Severity stripe (2px top bar, color from severity)
- Severity dot (pulsing for critical)
- RepoPill for primary repo
- Impact chain: `repo → impacted-repo-1 → impacted-repo-2` with arrow separators
- Severity badge + tag badges
- Question text (primary label)
- Agent info line: agent ID, goal, age, assigned reviewer
- Quick action buttons: Approve (green), Deny (red), Comment (neutral) — always visible

**DecisionCard (expanded):**
- Click body toggles expand (accordion, max one open)
- `DiffBlock`: syntax-highlighted diff lines with add/del coloring, file path header
- `LogBlock`: monospaced, colored by severity
- `ScreenshotBlock`: italic description text

**DecisionQueue header:**
- Total pending count
- Critical count (red text) if > 0

### 6.2 Wire decision data and actions

**Data wiring:**
- On mount: `api.decisions(workspaceId)` for initial load
- Subscribe to `/decisions/ws?workspace_id=` for real-time updates
- On `decision_created`: insert in correct sort position (severity desc, then age asc)
- On `decision_resolved`: animate card removal

**Action wiring:**
- Approve: `api.approve(id, { reviewer })` → on success, card animates out
- Deny: `api.deny(id, { reviewer })` → on success, card animates out
- Comment: expand inline text input, `api.comment(id, { reviewer, comment })` → on success, card animates out

**Card animation:**
- CSS transition on `max-height` + `opacity` for smooth removal
- `stopPropagation()` on action buttons to prevent card expand toggle

### 6.3 Update nav badge

- Decision pending count drives the badge on the "Decisions" nav tab
- Count updates in real-time from WS events
- Badge uses ember accent color

**Verification:** Decision queue loads with pending decisions, filtering works, approve/deny/comment work, cards animate out, nav badge updates.

---

## Phase 7: Frontend — Session Replay

**Goal:** Build the multi-modal session replay view with timeline, diffs, logs, and terminal tabs.

**Cross-cutting concerns addressed:** CC-1 (replay module), CC-5 (pagination), CC-9 (scrubbing replay data).

### 7.1 Add replay endpoints to hub (proxy from edge)

**File:** `crates/forgehub/src/main.rs`

Register routes with pagination support (CC-5):

```
GET /sessions/:id/replay/timeline?after=<event_id>&limit=100&level=summary|full
GET /sessions/:id/replay/diff
GET /sessions/:id/replay/terminal?offset=<byte>&limit=<bytes>
```

**For v1, all three proxy from the edge:**
- `/replay/timeline`: parse `replay.jsonl` from edge. `level=summary` returns only System, Decision, Switch, Test events (default). `level=full` includes Read, Edit, Tool. Cursor-based pagination via `after` param.
- `/replay/diff`: proxy `git diff --stat` from edge per touched repo (no pagination needed — diffs are bounded by session scope)
- `/replay/terminal`: proxy existing `/sessions/:id/logs` from edge with byte-range pagination

**File:** `crates/forgehub/src/lib.rs`

- `HubService::replay_timeline(session_id, after, limit, level) -> (Vec<ReplayEvent>, Option<String> /* next_cursor */)`
- `HubService::replay_diff(session_id) -> Vec<DiffGroup>`
- `HubService::replay_terminal(session_id, offset, limit) -> (String, u64 /* total_bytes */)`

### 7.2 Add replay event emission to edge

**File:** `crates/forged/src/lib.rs`

- Add `emit_replay_event(&self, session_id, event: ReplayEvent)`
- Appends structured JSONL to `{data_dir}/sessions/{session_id}/replay.jsonl`
- **Run `scrub()` on event action/result fields before writing (CC-9)**
- Emit events on: session start (system), file read, file edit, tool call, repo context switch, test run, decision request

**File:** `crates/forgemux-core/src/replay.rs` (new module, per CC-1)

- Add `ReplayEvent` and `ReplayEventType` types (already specified in tech_design_spec.md section 3.2)

**File:** `crates/forged/src/server.rs`

- Add `GET /sessions/:id/replay.jsonl` endpoint (returns raw JSONL file)

### 7.3 Build SessionReplay view

**File:** `dashboard/components/replay.js`

Port from `forgemux-hub.jsx` `SessionReplay`:

**TimelineSidebar (left pane, 300px):**
- Session header: status dot, session ID, model badge, goal, repo pills
- Vertical timeline of events
- Each event: timestamp, repo icon+color, action description
- Context switches: larger marker, colored connector line, bold text
- Decision events: red background
- Test events: green (pass) / red (fail) result marker
- Click event → scroll main pane to corresponding change

**TabBar:**
- Unified Diff | File Tree | Structured Log | Terminal
- Active tab: ember underline
- Atomic Merge CTA button (gradient, disabled for v1)

**Unified Diff tab:**
- Files grouped by repo header (icon, color, file count)
- File rows: path, additions (green), deletions (red)
- Click to expand full diff (future)

**File Tree tab:**
- Tree showing only repos session modified
- Repo top-level with icon + color
- Modified files highlighted
- Click file → open diff

**Structured Log tab:**
- Table: timestamp, event type icon, repo (icon + label), action, result badge
- Alternating row backgrounds
- Context switches and decisions visually emphasized

**Terminal tab:**
- Monospaced dark background
- Color-coded: ember prompts, green success, red errors, purple context switches, gray system info

### 7.4 Wire replay data

- On view mount (or session selection): fetch timeline, diff, and logs in parallel
- Tab switching renders the corresponding sub-view
- Timeline events use icons: system `◦`, read `◎`, edit `✎`, tool `⚡`, switch `⇋`, test `▷`, decision `⬡`

**Verification:** Select a session, replay view loads with timeline, diff groups render correctly, tab switching works, terminal shows color-coded output.

---

## Phase 8: Polish + Integration Testing

**Goal:** End-to-end wiring, keyboard shortcuts, error handling, and production hardening.

**Cross-cutting concerns addressed:** CC-2 (stale session handling in edge offline), CC-8 (tracing instrumentation audit across all new code paths).

### 8.1 End-to-end decision flow test

Test the full round trip:
1. Agent in tmux emits a question (via Claude Code hook or prompt pattern)
2. Forged detects it, POSTs to hub
3. Decision appears in dashboard WS
4. Reviewer clicks Approve in dashboard
5. Hub forwards to forged
6. Forged injects response into tmux
7. Agent continues

### 8.2 Error handling and edge cases

**Frontend:**
- API error handling: show inline error state, retry button
- WS disconnect: live indicator updates, auto-reconnect
- Empty states: "No active sessions", "No pending decisions", "No replay data"
- Loading states: skeleton or spinner while fetching

**Backend:**
- Edge offline: session cache retains last known state, mark as stale after N missed polls
- Decision for terminated session: return error, remove from queue
- Budget exhausted: decisions still created but dashboard shows warning

### 8.3 Session navigation

- Clicking a session row in FleetDashboard navigates to Session Replay for that session
- Clicking a session ID in a decision card navigates to its replay
- Browser back/forward works with hash routing

### 8.4 Keyboard shortcuts (future-ready wiring)

Wire event listeners in `app.js` for future activation:
- `1` / `2` / `3`: switch between Dashboard / Decisions / Replay tabs
- `a` / `d` / `c`: approve / deny / comment on focused decision card
- `Escape`: close expanded decision card

### 8.5 Legacy dashboard cutover

**File:** `crates/forgehub/src/main.rs`

- Serve new SPA from `dashboard/index.html` at `/`
- Move old dashboard to `/legacy` for fallback during transition
- Remove legacy route once new dashboard is validated

---

## Dependency Graph

```
Phase 1 (Storage + Workspace)
  │
  ├──→ Phase 2 (Decision Backend)
  │       │
  │       ├──→ Phase 4 (Edge Decision Flow)
  │       │
  │       └──→ Phase 6 (Decision Queue UI)  ← requires Phase 5
  │
  └──→ Phase 3 (Session Enrichment + Risk)
          │
          └──→ Phase 5 (SPA Foundation + Fleet Dashboard)
                  │
                  ├──→ Phase 6 (Decision Queue UI)
                  │
                  └──→ Phase 7 (Session Replay)

Phase 8 (Polish) ← requires all above
```

Phases 2 and 3 can run in parallel after Phase 1.
Phase 4 can start as soon as Phase 2 is complete.
Phases 5–7 are sequential (each builds on the SPA foundation).
Phase 6 requires both Phase 2 (backend) and Phase 5 (SPA shell).

---

## Files Changed/Created Per Phase

### Phase 1 — Hub Storage + Workspace
| Action | File | Notes |
|--------|------|-------|
| Refactor | `crates/forgemux-core/src/lib.rs` | Split into modules (CC-1) |
| Create | `crates/forgemux-core/src/session.rs` | Moved from lib.rs |
| Create | `crates/forgemux-core/src/state.rs` | Moved from lib.rs |
| Create | `crates/forgemux-core/src/repo.rs` | Moved from lib.rs |
| Create | `crates/forgemux-core/src/workspace.rs` | New types |
| Modify | `crates/forgehub/Cargo.toml` | sqlx, chrono-tz |
| Create | `crates/forgehub/migrations/001_init.sql` | Initial schema (CC-7) |
| Create | `crates/forgehub/src/db.rs` | DB init + workspace queries |
| Modify | `crates/forgehub/src/lib.rs` | Add db pool to HubService |
| Modify | `crates/forgehub/src/main.rs` | Workspace routes |

### Phase 2 — Decision Backend
| Action | File | Notes |
|--------|------|-------|
| Create | `crates/forgemux-core/src/decision.rs` | Decision types (CC-1) |
| Create | `crates/forgehub/migrations/002_decisions.sql` | If schema differs from 001 |
| Modify | `crates/forgehub/src/db.rs` | Decision CRUD + budget |
| Modify | `crates/forgehub/src/lib.rs` | Decision service + broadcast |
| Modify | `crates/forgehub/src/main.rs` | Decision routes + WS |

### Phase 3 — Session Enrichment + Risk
| Action | File | Notes |
|--------|------|-------|
| Create | `crates/forgemux-core/src/meta.rs` | SessionHubMeta, RiskLevel (CC-1) |
| Create | `crates/forgehub/src/risk.rs` | Risk scoring |
| Modify | `crates/forgehub/src/lib.rs` | Edge polling loop (CC-2) |
| Modify | `crates/forgehub/src/main.rs` | Extended WS payload |
| Modify | `crates/forgehub/src/db.rs` | Session cache + unreachable |

### Phase 4 — Edge Decision Flow
| Action | File | Notes |
|--------|------|-------|
| Create | `crates/forgemux-core/src/scrub.rs` | Credential scrubbing (CC-9) |
| Modify | `crates/forged/src/server.rs` | Decision endpoints (CC-3) |
| Modify | `crates/forged/src/lib.rs` | Tmux env vars, state detection |
| Modify | `crates/forgemux-core/src/state.rs` | Decision prompt patterns |

### Phase 5 — SPA Foundation + Fleet Dashboard
| Action | File | Notes |
|--------|------|-------|
| Create | `dashboard/index.html` | New SPA shell with font preload (CC-6) |
| Create | `dashboard/app.js` | Router, top nav |
| Create | `dashboard/theme.js` | T token object |
| Create | `dashboard/state.js` | Shared state |
| Create | `dashboard/lib/preact.module.js` | Vendored |
| Create | `dashboard/lib/htm.module.js` | Vendored |
| Create | `dashboard/lib/hooks.module.js` | Vendored |
| Create | `dashboard/components/shared.js` | With unreachable color (CC-2) |
| Create | `dashboard/components/nav.js` | TopNav |
| Create | `dashboard/components/fleet.js` | FleetDashboard |
| Create | `dashboard/services/api.js` | REST client |
| Create | `dashboard/services/ws.js` | WS with auto-reconnect |
| Modify | `crates/forgehub/src/main.rs` | Static file serving |

### Phase 6 — Decision Queue UI
| Action | File | Notes |
|--------|------|-------|
| Create | `dashboard/components/decisions.js` | Full decision queue |

### Phase 7 — Session Replay
| Action | File | Notes |
|--------|------|-------|
| Create | `crates/forgemux-core/src/replay.rs` | ReplayEvent types (CC-1) |
| Create | `dashboard/components/replay.js` | Multi-modal replay view |
| Modify | `crates/forgehub/src/main.rs` | Replay routes with pagination (CC-5) |
| Modify | `crates/forgehub/src/lib.rs` | Replay proxy methods |
| Modify | `crates/forged/src/lib.rs` | Replay event emission + scrub (CC-9) |
| Modify | `crates/forged/src/server.rs` | replay.jsonl endpoint |

### Phase 8 — Polish
| Action | File |
|--------|------|
| Modify | `dashboard/app.js` |
| Modify | `dashboard/components/*.js` |
| Modify | `crates/forgehub/src/main.rs` |

---

## Open Decisions to Resolve Before Starting

Resolved in code (documented here for completeness):

| # | Decision | Implemented | Notes |
|---|----------|-------------|-------|
| 1 | **Workspace seeding for v1** | **Default workspace seed in DB** | `ensure_workspace` inserts `org-default` + workspace on decision creation. |
| 2 | **Session-to-workspace mapping** | **All sessions default** | `workspace_id` is `"default"` in decision forward path; sessions list is not workspace-scoped. |
| 3 | **WS backward compatibility** | **Legacy flat array retained** | `/sessions/ws` still returns a flat session array; no `?v=2` gating yet. |
| 4 | **Reviewer identity** | **LocalStorage reviewer name** | Dashboard stores `forgemux_reviewer` locally; no token-derived identity yet. |
| 5 | **SQLite vs Postgres for v1** | **SQLite** | Hub uses sqlite migrations (`001_init.sql`). |
| 6 | **Heartbeat interval** | **3s poll** | Hub poll loop runs every 3s; unreachable after 2 misses. |
| 7 | **Credential scrubbing strictness** | **Strict patterns** | `scrub.rs` uses targeted patterns; validated in tests. |

---

## Test Plan

### Test Infrastructure Overview

**Current state:** The project has unit tests in `forgemux-core/src/lib.rs` only — session ID generation, store roundtrip, optimistic concurrency, state detection (4 scenarios), and session sorting. No integration tests, no property-based tests, no mutation testing, no CI pipeline, no coverage tooling.

**Target state:** Five-layer test strategy covering unit tests, property-based tests (QuickCheck), integration tests, mutation testing (cargo-mutants), and frontend component tests.

### Test Dependencies

Add to workspace `Cargo.toml` under `[workspace.dependencies]`:

```toml
quickcheck = "1"
quickcheck_macros = "1"
```

Add to each crate's `Cargo.toml` under `[dev-dependencies]`:

```toml
quickcheck = { workspace = true }
quickcheck_macros = { workspace = true }
tokio = { version = "1", features = ["test-util", "macros"] }   # forgehub, forged
axum-test = "0.16"                                                # forgehub, forged
```

Install tooling:

```bash
cargo install cargo-mutants
```

### Mutation Testing Configuration

**File:** `.cargo/mutants.toml`

```toml
exclude_globs = [
    "crates/fmux/**",              # CLI binary — tested via integration, not unit mutation
    "dashboard/**",                 # JS frontend — not Rust
    "crates/*/tests/**",            # Test code itself
]

exclude_re = [
    "impl.*Display.*fmt",          # Display impls are cosmetic
    "impl.*Debug.*fmt",            # Debug impls are cosmetic
    "fn main",                      # Entry points tested via integration
    "fn init_tracing",             # Logging setup
    "fn init_db",                   # DB bootstrap (tested by integration tests)
]

timeout_multiplier = 3.0
jobs = 4
```

---

### Layer 1: Unit Tests

Standard `#[test]` functions colocated with implementation code. Every new module gets unit tests as part of its implementation phase.

#### Phase 1 — Hub Storage + Workspace

**File:** `crates/forgemux-core/src/session.rs` (CC-1: tests move with module)

| Test | What it verifies |
|------|------------------|
| `module_split_reexports_work` | `use forgemux_core::SessionRecord` still resolves after split (CC-1) |
| `unreachable_state_serde` | `SessionState::Unreachable` serializes to `"unreachable"` and roundtrips (CC-2) |
| `unreachable_state_priority` | `state_priority(Unreachable)` sorts between Errored and Terminated (CC-2) |
| `session_record_goal_field_optional` | Deserializing JSON without `goal` yields `None` (CC-4 backward compat) |

**File:** `crates/forgemux-core/src/workspace.rs`

| Test | What it verifies |
|------|------------------|
| `workspace_serde_roundtrip` | Workspace serializes to JSON and deserializes back identically |
| `workspace_repo_serde_roundtrip` | WorkspaceRepo icon/color fields survive roundtrip |
| `attention_budget_serde_roundtrip` | AttentionBudget with timezone string roundtrips |
| `organization_serde_roundtrip` | Organization id/name roundtrip |

**File:** `crates/forgehub/src/db.rs`

| Test | What it verifies |
|------|------------------|
| `init_db_creates_tables` | All 6 tables exist after `init_db()` (via `PRAGMA table_info`) |
| `migrations_run_on_startup` | `sqlx::migrate!()` applies 001_init.sql correctly (CC-7) |
| `workspace_insert_and_get` | Seed workspace, retrieve by id, fields match |
| `workspace_list_returns_all` | Seed 3 workspaces, list returns all 3 |

#### Phase 2 — Decision Backend

**File:** `crates/forgemux-core/src/lib.rs`

| Test | What it verifies |
|------|------------------|
| `decision_serde_roundtrip_diff` | Decision with Diff context (file, lines) roundtrips |
| `decision_serde_roundtrip_log` | Decision with Log context roundtrips |
| `decision_serde_roundtrip_screenshot` | Decision with Screenshot context roundtrips |
| `severity_ordering` | `Critical < High < Medium < Low` (PartialOrd) |
| `decision_action_serde` | Approve/Deny/Comment serialize to expected snake_case strings |
| `diff_line_type_serde` | Ctx/Add/Del serialize correctly |

**File:** `crates/forgehub/src/db.rs`

| Test | What it verifies |
|------|------------------|
| `insert_and_get_decision` | Insert decision, get by id, all fields match |
| `list_decisions_filters_by_workspace` | Decisions in workspace A not returned when querying workspace B |
| `list_decisions_filters_by_repo` | Repo filter returns only matching decisions |
| `list_decisions_filters_by_status` | `status=pending` excludes resolved decisions |
| `resolve_decision_sets_resolution` | After resolve, resolution fields populated |
| `resolve_decision_logs_to_budget` | Resolving inserts row into `attention_budget_log` |
| `budget_used_today_counts_correctly` | 3 actions today + 2 yesterday = `used_today == 3` |
| `budget_used_today_respects_timezone` | Actions near midnight boundary in non-UTC timezone |
| `resolve_nonexistent_decision_returns_error` | Resolving `D-9999` returns error |
| `double_resolve_returns_error` | Resolving already-resolved decision returns error |

#### Phase 3 — Risk Scoring

**File:** `crates/forgehub/src/risk.rs`

| Test | What it verifies |
|------|------------------|
| `green_when_all_healthy` | Context 50%, tests passing, no pending decisions -> Green |
| `yellow_at_context_70` | Exactly 70% context -> Yellow |
| `yellow_when_tests_failing` | Context 50%, tests failing -> Yellow |
| `yellow_when_decision_pending_5_to_15_min` | Pending decision at 10 min -> Yellow |
| `red_at_context_86` | Exactly 86% context -> Red |
| `red_when_decision_pending_over_15_min` | Pending decision at 20 min -> Red |
| `red_when_errored` | Errored session -> Red |
| `red_takes_priority_over_yellow` | Context 86% + tests failing -> Red (not Yellow) |
| `resolved_decisions_not_counted` | Decision with `resolved_at` set does not affect risk |
| `context_69_is_green` | Boundary: 69% -> Green |
| `context_85_is_yellow` | Boundary: 85% -> Yellow |
| `decision_at_4m59s_is_green` | Just under 5m threshold -> Green |
| `decision_at_5m_is_yellow` | Exactly 5m -> Yellow |
| `decision_at_14m59s_is_yellow` | Just under 15m threshold -> Yellow |
| `decision_at_15m_is_red` | Exactly 15m -> Red |

#### Phase 3 — Edge Heartbeat + Unreachable (CC-2)

**File:** `crates/forgehub/src/lib.rs`

| Test | What it verifies |
|------|------------------|
| `edge_poll_success_resets_failures` | After failure + success, `consecutive_failures == 0` |
| `two_consecutive_failures_marks_unreachable` | 2 poll failures -> all edge sessions become `Unreachable` |
| `one_failure_does_not_mark_unreachable` | 1 poll failure -> sessions retain original state |
| `recovery_reverts_unreachable_sessions` | Success after Unreachable -> sessions revert to actual state |

**File:** `crates/forgehub/src/db.rs`

| Test | What it verifies |
|------|------------------|
| `mark_edge_sessions_unreachable_returns_count` | Returns correct count of affected sessions |
| `mark_edge_sessions_unreachable_scoped_to_edge` | Sessions on other edges unaffected |

#### Phase 4 — Edge Decision Flow

**File:** `crates/forgemux-core/src/scrub.rs` (CC-9)

| Test | What it verifies |
|------|------------------|
| `scrub_aws_key` | `AKIAIOSFODNN7EXAMPLE` replaced with `[REDACTED]` |
| `scrub_generic_token` | `token=abc123...long_hex` redacted |
| `scrub_private_key` | `-----BEGIN RSA PRIVATE KEY-----` block redacted |
| `scrub_connection_string` | `postgres://user:pass@host` password redacted |
| `scrub_preserves_normal_text` | Non-sensitive text passes through unchanged |
| `scrub_multiple_patterns` | Input with multiple credential types all redacted in single pass |
| `scrub_empty_string` | Empty input returns empty output |

**File:** `crates/forged/src/server.rs`

| Test | What it verifies |
|------|------------------|
| `create_decision_returns_201` | Valid POST creates decision successfully |
| `create_decision_rejects_unknown_session` | Nonexistent session -> 404 |
| `create_decision_scrubs_context` | Decision context with AWS key is scrubbed before forwarding (CC-9) |
| `decision_response_formats_approval_text` | Approval generates correct injection text |
| `decision_response_formats_denial_text` | Denial generates correct injection text |

---

### Layer 2: Property-Based Tests (QuickCheck)

Property-based tests verify that invariants hold across hundreds of randomized inputs. They catch edge cases that hand-written tests miss — malformed strings, boundary integers, empty collections, unusual enum combinations.

#### Custom Arbitrary Implementations

**File:** `crates/forgemux-core/src/lib.rs` (inside `#[cfg(test)]`)

Implement `Arbitrary` for all enums so QuickCheck can generate them:

```rust
use quickcheck::{Arbitrary, Gen};

impl Arbitrary for Severity {
    fn arbitrary(g: &mut Gen) -> Self {
        match u8::arbitrary(g) % 4 {
            0 => Severity::Critical, 1 => Severity::High,
            2 => Severity::Medium,   _ => Severity::Low,
        }
    }
}

impl Arbitrary for RiskLevel {
    fn arbitrary(g: &mut Gen) -> Self {
        match u8::arbitrary(g) % 3 {
            0 => RiskLevel::Green, 1 => RiskLevel::Yellow, _ => RiskLevel::Red,
        }
    }
}

impl Arbitrary for SessionState {
    fn arbitrary(g: &mut Gen) -> Self {
        match u8::arbitrary(g) % 8 {
            0 => SessionState::Provisioning, 1 => SessionState::Starting,
            2 => SessionState::Running,      3 => SessionState::Idle,
            4 => SessionState::WaitingInput, 5 => SessionState::Errored,
            6 => SessionState::Unreachable,  _ => SessionState::Terminated,
        }
    }
}

// Same pattern for: TestsStatus, DiffLineType, DecisionAction
```

#### Property Tests — Core Types

**File:** `crates/forgemux-core/src/lib.rs`

| Property | Invariant |
|----------|-----------|
| `prop_severity_serde_roundtrip(s: Severity)` | `deserialize(serialize(s)) == s` for all values |
| `prop_risk_level_serde_roundtrip(r: RiskLevel)` | Same for RiskLevel |
| `prop_session_state_serde_roundtrip(s: SessionState)` | Same for all 8 states (including Unreachable) |
| `prop_decision_action_serde_roundtrip(a: DecisionAction)` | Same for Approve/Deny/Comment |
| `prop_tests_status_serde_roundtrip(t: TestsStatus)` | Same for all 4 test statuses |
| `prop_severity_ordering_is_total(a: Severity, b: Severity)` | Exactly one of `a < b`, `a == b`, `a > b` holds |
| `prop_severity_ordering_transitive(a, b, c: Severity)` | If `a <= b` and `b <= c` then `a <= c` |
| `prop_session_id_always_has_prefix(_: u8)` | Every SessionId starts with `"S-"` and is 10 chars |
| `prop_session_ids_are_unique(_: u16)` | Two consecutive `SessionId::new()` never collide |
| `prop_session_store_save_load_roundtrip(state: SessionState)` | Save with any state, load back, state matches |
| `prop_sort_sessions_is_idempotent(states: Vec<SessionState>)` | `sort(sort(x)) == sort(x)` |
| `prop_sort_sessions_waiting_input_first(states: Vec<SessionState>)` | WaitingInput always has lower index than Running |
| `prop_dead_process_never_running(exit_code: Option<i32>)` | `process_alive=false` -> Terminated or Errored, never Running |
| `prop_waiting_hint_always_wins(idle_secs: u16)` | `waiting_hint=true` -> WaitingInput regardless of idle duration |

#### Property Tests — Risk Scoring

**File:** `crates/forgehub/src/risk.rs`

| Property | Invariant |
|----------|-----------|
| `prop_risk_is_deterministic(context_pct: u8, failing: bool)` | Same inputs always produce same RiskLevel |
| `prop_higher_context_never_decreases_risk(low: u8, high: u8)` | `risk(high) >= risk(low)` when `low < high` (monotonic) |
| `prop_context_over_85_always_red(pct: u8)` | Any `pct > 85` -> Red regardless of other factors |
| `prop_context_under_70_passing_no_decisions_green(pct: u8)` | `pct < 70` + passing + no pending -> Green |

These properties encode the *design invariants* of risk scoring, not just specific cases. If a future refactor introduces a threshold regression, these catch it.

#### Property Tests — Stream Protocol

**File:** `crates/forged/src/stream.rs`

| Property | Invariant |
|----------|-----------|
| `prop_event_ring_never_exceeds_capacity(events: Vec<String>)` | `ring.len() <= capacity` after any sequence of pushes |
| `prop_events_since_returns_only_newer(count: u8, since: u8)` | All returned events have `id > since` |
| `prop_input_deduper_rejects_duplicates(id: String)` | First `accept(id)` -> true, second -> false |
| `prop_input_deduper_accepts_all_unique(ids: Vec<u16>)` | Deduplicated set all accepted on first call |
| `prop_latest_event_id_equals_last_pushed(events: Vec<u8>)` | After pushing 0..N, `latest_event_id() == N-1` |

#### Property Tests — Attention Budget

**File:** `crates/forgehub/src/db.rs`

| Property | Invariant |
|----------|-----------|
| `prop_budget_used_equals_log_count(count: u8)` | Insert N budget logs today, `budget_used_today() == N` |

---

### Layer 3: Integration Tests

Integration tests verify component interactions through the public API. They live in `crates/*/tests/` and use `axum-test` to spin up real HTTP servers with in-memory SQLite.

#### Test Helper

**File:** `crates/forgehub/tests/helpers/mod.rs`

```rust
pub async fn setup_test_hub() -> TestServer {
    // In-memory SQLite pool
    // Seed one org + workspace with 4 repos
    // Build axum Router with test HubService
    // Return axum-test TestServer
}

pub fn make_test_decision(repo: &str, severity: Severity) -> Decision {
    // Factory with sensible defaults
}
```

#### Hub API Integration Tests

**File:** `crates/forgehub/tests/api_integration.rs`

| Test | Flow |
|------|------|
| `test_workspace_list` | GET /workspaces -> contains seeded workspace |
| `test_workspace_get_with_budget` | GET /workspaces/:id -> includes budget with used count |
| `test_decision_full_lifecycle` | POST decision -> GET (present) -> POST approve -> GET pending (absent) -> budget incremented |
| `test_decision_filtering_by_repo` | Create in 2 repos -> filter by repo -> correct subset |
| `test_decision_filtering_by_status` | Create + resolve one -> filter pending -> only unresolved |
| `test_decision_sort_order` | Create critical + low -> GET -> critical first |
| `test_sessions_with_hub_meta` | Seed cache -> GET /sessions -> hub_meta with risk, context_pct |
| `test_sessions_stats_computed` | Seed mixed sessions -> stats.active, stats.blocked correct |
| `test_auth_rejects_missing_token` | GET /decisions no auth -> 401 |
| `test_auth_rejects_invalid_token` | GET /decisions bad token -> 401 |
| `test_replay_timeline` | Seed events -> GET /replay/timeline -> ordered by timestamp |
| `test_replay_diff_grouped_by_repo` | Seed multi-repo diffs -> groups correct |

#### Hub WebSocket Integration Tests

**File:** `crates/forgehub/tests/ws_integration.rs`

| Test | Flow |
|------|------|
| `test_decisions_ws_receives_created` | Connect WS -> create via REST -> WS gets `decision_created` |
| `test_decisions_ws_receives_resolved` | Create + resolve -> WS gets `decision_resolved` |
| `test_sessions_ws_v2_structured` | Connect ?v=2 -> update cache -> WS gets `session_update` with hub_meta |
| `test_sessions_ws_v1_backward_compat` | Connect without v=2 -> receives flat session array |
| `test_ws_initial_payload` | Connect /decisions/ws -> first message is current pending decisions |

#### Edge Decision Flow Integration Tests

**File:** `crates/forged/tests/decision_flow.rs`

| Test | Flow |
|------|------|
| `test_decision_creates_and_forwards` | Forged with mock hub -> POST decision -> hub received it |
| `test_decision_response_injects_input` | Forged with tmux session -> POST response -> pane shows injection |
| `test_decision_for_unknown_session_404` | POST /sessions/bad-id/decision -> 404 |

---

### Layer 4: Mutation Testing (cargo-mutants)

Mutation testing validates test suite quality. cargo-mutants applies source-level mutations (operator swaps, return value replacements, constant changes) to a scratch copy of the source, runs `cargo test`, and classifies each mutant as **killed** (tests caught it) or **survived** (test gap).

#### Targeted Runs Per Phase

| Phase | Command | Why |
|-------|---------|-----|
| 1 | `cargo mutants -f crates/forgehub/src/db.rs` | Verify DB CRUD tests catch SQL mutations |
| 2 | `cargo mutants -f crates/forgemux-core/src/lib.rs -F "Decision\|Severity\|DecisionAction"` | Decision type serde and ordering |
| 2 | `cargo mutants -f crates/forgehub/src/db.rs -F "decision\|budget"` | Decision DB logic and budget counting |
| **3** | **`cargo mutants -f crates/forgehub/src/risk.rs`** | **Highest priority.** Thresholds (70%, 85%, 5m, 15m) where off-by-one has direct user impact |
| 4 | `cargo mutants -f crates/forgemux-core/src/scrub.rs` | Credential scrubbing regex patterns (CC-9) |
| 4 | `cargo mutants -f crates/forged/src/server.rs -F "decision"` | Edge decision endpoints |
| Baseline | `cargo mutants -f crates/forged/src/stream.rs` | Existing stream protocol — establish baseline |
| Baseline | `cargo mutants -f crates/forgemux-core/src/lib.rs -F "StateDetector\|sort_sessions\|SessionStore"` | Existing core logic — establish baseline |

#### Target Mutation Scores

| Module | Target killed % | Rationale |
|--------|----------------|-----------|
| `risk.rs` | **> 95%** | Critical business logic with exact numeric thresholds. Every comparison operator must be killed by a boundary test. |
| `db.rs` (decision CRUD) | > 85% | Data integrity. SQL WHERE clause mutations must be caught. |
| `stream.rs` (ring + dedup) | > 85% | Protocol correctness. Capacity overflow and dedup bypass are real bugs. |
| `forgemux-core` (state detection) | > 80% | State machine correctness. |
| `scrub.rs` (credential patterns) | > 90% | Security-critical. Regex bypass is a real vulnerability. |
| `server.rs` (endpoints) | > 70% | HTTP plumbing generates more equivalent mutants. |

#### Example: Why Mutation Testing Matters for risk.rs

Consider this line:

```rust
if meta.context_pct > 85 { return RiskLevel::Red; }
```

cargo-mutants generates these mutants:

| # | Mutant | Killed by |
|---|--------|-----------|
| 1 | `context_pct >= 85` (boundary shift) | `context_85_is_yellow` — expects Yellow, mutant returns Red |
| 2 | `context_pct > 0` (constant change) | `green_when_all_healthy` — context 50%, expects Green, gets Red |
| 3 | `context_pct < 85` (operator flip) | `red_at_context_86` — expects Red, gets Green |
| 4 | Replace body with `RiskLevel::Green` | `red_at_context_86` — expects Red, gets Green |
| 5 | Replace body with `RiskLevel::Yellow` | `red_at_context_86` — expects Red, gets Yellow |

Without the boundary test at exactly 85%, **mutant 1 survives**. This is the specific class of bug mutation testing is designed to find.

#### Interpreting Results

After each run, inspect `mutants.out/`:
- `caught.txt` — killed (good)
- `missed.txt` — survived (test gaps)
- `timeout.txt` — caused infinite loops
- `unviable.txt` — did not compile
- `outcomes.json` — machine-readable full results

**Triage guide for survivors:**

| Survived in | Action |
|-------------|--------|
| `risk.rs` threshold comparison | **Must fix.** Add boundary unit test. |
| `risk.rs` returning different RiskLevel | **Must fix.** Add property test. |
| `db.rs` SQL WHERE clause removed | **Must fix.** Integration test missing filter assertion. |
| `db.rs` different error type | Review. May be equivalent if caller handles all errors the same. |
| `stream.rs` capacity check | **Must fix.** Property test should catch this. |
| `server.rs` different HTTP status | Review. 400 vs 422 may be acceptable. 200 vs 404 must fix. |
| Any `Display::fmt` | Ignore. Cosmetic. |

#### Weekly Full Audit

```bash
cargo mutants \
  -f crates/forgemux-core/src/lib.rs \
  -f crates/forgemux-core/src/scrub.rs \
  -f crates/forgehub/src/risk.rs \
  -f crates/forgehub/src/db.rs \
  -f crates/forged/src/stream.rs \
  -j 4 --timeout 120

# Archive for tracking
cp -r mutants.out/ mutants-$(date +%Y%m%d)/
```

Track `killed / (killed + missed)` ratio over time. Ratchet: never let it decrease between releases.

---

### Layer 5: Frontend Tests

The Preact+HTM frontend has no build step. Tests use lightweight Node-based assertions.

#### Component Logic Tests

**File:** `dashboard/tests/components.test.js`

```js
import { riskColor, statusColor, contextColor, severityColor } from '../components/shared.js';
import { T } from '../theme.js';

// Theme token integrity
assert(T.ember === '#E8622C');
assert(T.ok === '#34D399');

// Context color thresholds (must mirror risk.rs boundaries)
assert(contextColor(0) === T.ok);       // < 40 -> green
assert(contextColor(39) === T.ok);
assert(contextColor(40) === T.molten);  // 40-69 -> amber
assert(contextColor(69) === T.molten);
assert(contextColor(70) === T.warn);    // 70-84 -> yellow
assert(contextColor(84) === T.warn);
assert(contextColor(85) === T.err);     // >= 85 -> red
assert(contextColor(100) === T.err);

// Severity, status, risk color mappings
assert(severityColor('critical') === T.errD);
assert(statusColor('active') === T.ok);
assert(statusColor('unreachable') === T.t4);  // CC-2: greyed out
assert(riskColor('red') === T.err);
```

#### Decision Sort + API Client Tests

**Files:** `dashboard/tests/decisions.test.js`, `dashboard/tests/api.test.js`

- Decisions sort: critical before high before medium before low, oldest first within severity
- fetchJSON adds Authorization header when token present
- api.approve sends POST with correct body shape

#### Run Frontend Tests

```bash
node --experimental-vm-modules dashboard/tests/components.test.js
node --experimental-vm-modules dashboard/tests/decisions.test.js
node --experimental-vm-modules dashboard/tests/api.test.js
```

---

### Test Execution Summary

| Context | What runs | Frequency |
|---------|-----------|-----------|
| Developer loop | `cargo test --workspace` (unit + property + integration) | Every commit |
| Pre-merge CI | `cargo test --workspace` + `cargo clippy -- -D warnings` + frontend tests | Every PR |
| Weekly audit | `cargo mutants -j 4 --timeout 120` on critical modules, archive `outcomes.json` | Weekly |
| Pre-release | Full `cargo mutants` run, verify 0 survivors in `risk.rs` | Each release |

---

### Test Matrix By Phase

| Phase | Unit | Property (QC) | Integration | Mutation Target |
|-------|------|---------------|-------------|-----------------|
| **1 — Storage** | Module split verify (1), Unreachable serde/priority (2), goal backward compat (1), Workspace serde (4), DB init/CRUD (4) | — | Workspace API (2) | `db.rs` |
| **2 — Decisions** | Decision serde (6), DB CRUD (10) | Severity ordering (2), serde roundtrips (5), budget (1) | Decision lifecycle (4), WS events (3) | `db.rs` decision/budget fns |
| **3 — Risk** | Threshold boundaries (15), edge heartbeat (4), DB unreachable (2) | Determinism (1), monotonicity (1), context thresholds (2) | Sessions enriched (2) | **`risk.rs` (highest priority)** |
| **4 — Edge Flow** | Scrub patterns (7), endpoint validation (5) | — | Decision forward + inject (3) | `scrub.rs`, `server.rs` decision fns |
| **5 — Fleet UI** | Theme tokens, color helpers incl. unreachable (JS) | — | — | — |
| **6 — Decision UI** | Sort/filter logic (JS) | — | — | — |
| **7 — Replay** | Replay event serde | Ring + dedup properties (5) | Timeline ordering, diff grouping (2) | `stream.rs` |
| **8 — Polish** | — | — | End-to-end decision flow (1) | Full audit (all modules) |

### Estimated Test Counts

| Layer | Count | Notes |
|-------|-------|-------|
| Unit tests (Rust) | ~75 new | In addition to existing ~12 in forgemux-core. Includes scrub.rs (7), heartbeat (6), Unreachable (3), module split (1) |
| Property tests (QuickCheck) | ~22 | Core types (incl. Unreachable state), risk scoring, stream protocol |
| Integration tests (axum-test) | ~18 | Hub API, Hub WS, edge decision flow |
| Frontend tests (Node) | ~20 | Theme, color helpers (incl. unreachable), sort logic, API client |
| Mutation targets | 6 modules | risk.rs, db.rs, stream.rs, scrub.rs, core/lib.rs, server.rs |
