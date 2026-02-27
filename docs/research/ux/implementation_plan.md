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

## Phase 1: Hub Storage Foundation + Workspace Model

**Goal:** Add SQLite persistence to forgehub and introduce the Organization/Workspace/Repo hierarchy.

### 1.1 Add sqlx + chrono-tz dependencies

**File:** `crates/forgehub/Cargo.toml`

- Add `sqlx = { version = "0.7", features = ["runtime-tokio", "sqlite", "postgres", "chrono"] }`
- Add `chrono-tz = "0.9"`

**Verification:** `cargo check -p forgehub` compiles.

### 1.2 Define Organization, Workspace, WorkspaceRepo types

**File:** `crates/forgemux-core/src/lib.rs`

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

**Verification:** Unit tests for serde roundtrip.

### 1.3 Create SQL schema and DB init

**File:** `crates/forgehub/src/db.rs` (new)

- Create `init_db(data_dir: &Path) -> Result<SqlitePool>`
- Runs `CREATE TABLE IF NOT EXISTS` for: `organizations`, `workspaces`, `decisions`, `attention_budget_log`, `replay_events`, `session_cache`
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

**File:** `crates/forgemux-core/src/lib.rs`

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

### 3.1 Define SessionHubMeta and related types

**File:** `crates/forgemux-core/src/lib.rs`

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

### 3.3 Implement edge polling loop with session cache

**File:** `crates/forgehub/src/lib.rs`

- Add background tokio task: `poll_edges(hub: Arc<HubService>)`
- Every 3 seconds, for each registered edge:
  - `GET /sessions` from edge
  - Merge into `session_cache` table via `db.rs`
  - Compute risk scores (requires pending decisions from DB)
  - If anything changed, broadcast `SessionEvent::Updated` via `session_tx`
  - Recompute workspace stats, broadcast `SessionEvent::StatsChanged` if changed
  - Recompute attention budget, broadcast `SessionEvent::BudgetChanged` if changed

**File:** `crates/forgehub/src/db.rs`

- `upsert_session_cache(pool, session_id, workspace_id, edge_id, meta_json, state) -> Result<()>`
- `list_cached_sessions(pool, workspace_id) -> Result<Vec<CachedSession>>`

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

### 4.1 Add decision endpoints to forged

**File:** `crates/forged/src/server.rs`

Register two new routes:

```
POST /sessions/:id/decision           # Agent/sidecar emits a decision request
POST /sessions/:id/decision-response  # Hub forwards reviewer's action
```

**`POST /sessions/:id/decision`:**
- Accepts `CreateDecisionPayload { question, context, severity, tags, impact_repo_ids }`
- Enriches with session metadata (agent_goal, repo, workspace)
- Forwards to hub: `POST /decisions` on forgehub

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

### 7.1 Add replay endpoints to hub (proxy from edge)

**File:** `crates/forgehub/src/main.rs`

Register routes:

```
GET /sessions/:id/replay/timeline
GET /sessions/:id/replay/diff
GET /sessions/:id/replay/terminal
```

**For v1, all three proxy from the edge:**
- `/replay/timeline`: parse `replay.jsonl` from edge (if exists), else return empty
- `/replay/diff`: proxy `git diff --stat` from edge per touched repo
- `/replay/terminal`: proxy existing `/sessions/:id/logs` from edge

**File:** `crates/forgehub/src/lib.rs`

- `HubService::replay_timeline(session_id) -> Vec<ReplayEvent>`
- `HubService::replay_diff(session_id) -> Vec<DiffGroup>`
- `HubService::replay_terminal(session_id) -> String`

### 7.2 Add replay event emission to edge

**File:** `crates/forged/src/lib.rs`

- Add `emit_replay_event(&self, session_id, event: ReplayEvent)`
- Appends structured JSONL to `{data_dir}/sessions/{session_id}/replay.jsonl`
- Emit events on: session start (system), file read, file edit, tool call, repo context switch, test run, decision request

**File:** `crates/forgemux-core/src/lib.rs`

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
| Action | File |
|--------|------|
| Modify | `crates/forgehub/Cargo.toml` |
| Modify | `crates/forgemux-core/src/lib.rs` |
| Create | `crates/forgehub/src/db.rs` |
| Modify | `crates/forgehub/src/lib.rs` |
| Modify | `crates/forgehub/src/main.rs` |

### Phase 2 — Decision Backend
| Action | File |
|--------|------|
| Modify | `crates/forgemux-core/src/lib.rs` |
| Modify | `crates/forgehub/src/db.rs` |
| Modify | `crates/forgehub/src/lib.rs` |
| Modify | `crates/forgehub/src/main.rs` |

### Phase 3 — Session Enrichment + Risk
| Action | File |
|--------|------|
| Modify | `crates/forgemux-core/src/lib.rs` |
| Create | `crates/forgehub/src/risk.rs` |
| Modify | `crates/forgehub/src/lib.rs` |
| Modify | `crates/forgehub/src/main.rs` |
| Modify | `crates/forgehub/src/db.rs` |

### Phase 4 — Edge Decision Flow
| Action | File |
|--------|------|
| Modify | `crates/forged/src/server.rs` |
| Modify | `crates/forged/src/lib.rs` |
| Modify | `crates/forgemux-core/src/lib.rs` |

### Phase 5 — SPA Foundation + Fleet Dashboard
| Action | File |
|--------|------|
| Create | `dashboard/index.html` (replace existing) |
| Create | `dashboard/app.js` |
| Create | `dashboard/theme.js` |
| Create | `dashboard/state.js` |
| Create | `dashboard/lib/preact.module.js` |
| Create | `dashboard/lib/htm.module.js` |
| Create | `dashboard/lib/hooks.module.js` |
| Create | `dashboard/components/shared.js` |
| Create | `dashboard/components/nav.js` |
| Create | `dashboard/components/fleet.js` |
| Create | `dashboard/services/api.js` |
| Create | `dashboard/services/ws.js` |
| Modify | `crates/forgehub/src/main.rs` |

### Phase 6 — Decision Queue UI
| Action | File |
|--------|------|
| Create | `dashboard/components/decisions.js` |

### Phase 7 — Session Replay
| Action | File |
|--------|------|
| Create | `dashboard/components/replay.js` |
| Modify | `crates/forgehub/src/main.rs` |
| Modify | `crates/forgehub/src/lib.rs` |
| Modify | `crates/forged/src/lib.rs` |
| Modify | `crates/forged/src/server.rs` |
| Modify | `crates/forgemux-core/src/lib.rs` |

### Phase 8 — Polish
| Action | File |
|--------|------|
| Modify | `dashboard/app.js` |
| Modify | `dashboard/components/*.js` |
| Modify | `crates/forgehub/src/main.rs` |

---

## Open Decisions to Resolve Before Starting

| # | Decision | Options | Suggested |
|---|----------|---------|-----------|
| 1 | **Workspace seeding for v1** — workspaces need to exist before the dashboard works. How to create them? | (a) TOML config file, (b) CLI command `fmux workspace create`, (c) REST API `POST /workspaces` | **(a)** TOML config — simplest for v1, no new CLI or API surface |
| 2 | **Session-to-workspace mapping** — how does the hub know which workspace a session belongs to? | (a) Agent passes workspace_id at start, (b) Map by repo path, (c) All sessions on an edge belong to one workspace | **(b)** Map by repo path — workspace config includes repo paths, hub matches on session's `repo_root` |
| 3 | **WS backward compatibility** — the current dashboard relies on flat session array from `/sessions/ws`. Break it? | (a) Break immediately, (b) Gate on `?v=2` query param, (c) Send both formats temporarily | **(b)** Gate on `?v=2` — new dashboard sends `?v=2`, legacy gets flat array |
| 4 | **Reviewer identity** — how does the dashboard know who the current user is for decision actions? | (a) Prompt on action, (b) Derive from auth token, (c) Set in localStorage on first visit | **(b)** Derive from auth token — pairing flow already associates identity |
