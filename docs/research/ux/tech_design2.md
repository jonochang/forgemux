# Forgemux Hub — Detailed Technology Design

**Source inputs:** `docs/research/ux/forgemux-hub-stories.md`, `docs/research/ux/forgemux-hub.jsx`
**Date:** 2026-02-27
**Supersedes:** `tech_design.md` (retained as overview; this doc adds implementation detail)

---

## 1. Purpose

This document specifies the concrete backend and frontend changes required to deliver the Forgemux Hub dashboard described in the user stories and JSX mockup. It maps each feature to Rust types, API endpoints, storage schemas, WebSocket messages, and frontend components. It assumes familiarity with the existing architecture (`docs/specs/design.md`) and the current implementation state (`PR_REVIEW.md`).

---

## 2. System Context

```
                    ┌──────────────────────────────────┐
                    │         Browser (SPA)            │
                    │  Fleet · Decisions · Replay      │
                    └──────┬──────────┬────────────────┘
                           │ HTTP     │ WebSocket
                           v          v
                    ┌──────────────────────────────────┐
                    │         forgehub (Hub)            │
                    │  REST API · WS broker · SQLite   │
                    │  Decision store · Replay cache   │
                    └──────┬──────────┬────────────────┘
                           │ HTTP     │ WS relay
                           v          v
            ┌──────────────────┐  ┌──────────────────┐
            │  forged (edge A) │  │  forged (edge B) │
            │  tmux · sessions │  │  tmux · sessions │
            └──────────────────┘  └──────────────────┘
```

The SPA never talks directly to edge nodes. All traffic goes through the hub.

---

## 3. Data Model Changes

### 3.1 New Rust Types (forgemux-core)

The following types extend `forgemux-core/src/lib.rs`. All are `Serialize + Deserialize + Clone`.

```rust
// --- Organization & Workspace ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Organization {
    pub id: String,       // e.g. "org-silverpond"
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceRepo {
    pub id: String,       // e.g. "payment-gateway"
    pub label: String,
    pub icon: String,     // single char: "◐", "◈", "◇", "◎"
    pub color: String,    // hex: "#34D399"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttentionBudget {
    pub used: u32,
    pub total: u32,
    pub reset_tz: String, // IANA timezone, e.g. "Australia/Melbourne"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub org_id: String,
    pub name: String,
    pub repos: Vec<WorkspaceRepo>,
    pub members: Vec<String>,
    pub attention_budget: AttentionBudget,
}

// --- Decision ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffLine {
    pub line_type: DiffLineType, // "ctx" | "add" | "del"
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffLineType { Ctx, Add, Del }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DecisionContext {
    Diff { file: String, lines: Vec<DiffLine> },
    Log { text: String },
    Screenshot { description: String },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Critical = 0,
    High = 1,
    Medium = 2,
    Low = 3,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    pub id: String,                        // "D-XXXX"
    pub session_id: String,
    pub workspace_id: String,
    pub repo_id: String,
    pub question: String,
    pub context: DecisionContext,
    pub severity: Severity,
    pub tags: Vec<String>,
    pub impact_repo_ids: Vec<String>,      // cross-repo cascade
    pub assigned_to: Option<String>,
    pub agent_goal: String,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub resolution: Option<DecisionResolution>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionResolution {
    pub action: DecisionAction,
    pub reviewer: String,
    pub comment: Option<String>,
    pub resolved_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionAction { Approve, Deny, Comment }

// --- Extended Session Fields ---
// Add to existing SessionRecord:

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionHubMeta {
    pub workspace_id: String,
    pub goal: String,
    pub risk: RiskLevel,
    pub context_pct: u8,
    pub touched_repos: Vec<String>,
    pub pending_decisions: u32,
    pub tests_status: TestsStatus,
    pub tokens_total: String,
    pub estimated_cost_usd: f64,
    pub lines_added: u32,
    pub lines_removed: u32,
    pub commits: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel { Green, Yellow, Red }

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestsStatus { Passing, Failing, Pending, None }

// --- Replay Event ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayEvent {
    pub id: u64,
    pub session_id: String,
    pub repo_id: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub elapsed: String,            // "0:14" display format
    pub event_type: ReplayEventType,
    pub action: String,
    pub result: Option<String>,     // "pass" | "fail"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayEventType {
    System, Read, Edit, Tool, Switch, Test, Decision,
}
```

### 3.2 SQL Schema (Hub Storage)

Hub storage lives at `{hub_data_dir}/hub.db` in SQLite for dev/test, and can be
backed by Postgres in production. The hub uses `sqlx` so the same schema and
queries can run against both engines.

```sql
CREATE TABLE organizations (
    id   TEXT PRIMARY KEY,
    name TEXT NOT NULL
);

CREATE TABLE workspaces (
    id                    TEXT PRIMARY KEY,
    org_id                TEXT NOT NULL REFERENCES organizations(id),
    name                  TEXT NOT NULL,
    timezone              TEXT NOT NULL DEFAULT 'UTC',
    attention_budget_total INTEGER NOT NULL DEFAULT 12,
    repos_json            TEXT NOT NULL DEFAULT '[]',  -- JSON array of WorkspaceRepo
    members_json          TEXT NOT NULL DEFAULT '[]'
);

CREATE TABLE decisions (
    id                TEXT PRIMARY KEY,
    session_id        TEXT NOT NULL,
    workspace_id      TEXT NOT NULL REFERENCES workspaces(id),
    repo_id           TEXT NOT NULL,
    question          TEXT NOT NULL,
    context_json      TEXT NOT NULL,       -- serialized DecisionContext
    severity          TEXT NOT NULL,       -- "critical"|"high"|"medium"|"low"
    tags_json         TEXT NOT NULL DEFAULT '[]',
    impact_repo_ids   TEXT NOT NULL DEFAULT '[]',
    assigned_to       TEXT,
    agent_goal        TEXT NOT NULL,
    created_at        TEXT NOT NULL,       -- ISO 8601
    resolved_at       TEXT,
    resolution_json   TEXT                 -- serialized DecisionResolution
);

CREATE INDEX idx_decisions_workspace ON decisions(workspace_id, resolved_at);
CREATE INDEX idx_decisions_severity  ON decisions(severity, created_at);

CREATE TABLE attention_budget_log (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    decision_id  TEXT NOT NULL,
    reviewer     TEXT NOT NULL,
    action       TEXT NOT NULL,
    logged_at    TEXT NOT NULL
);

CREATE TABLE replay_events (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    repo_id    TEXT,
    timestamp  TEXT NOT NULL,
    elapsed    TEXT NOT NULL,
    event_type TEXT NOT NULL,
    action     TEXT NOT NULL,
    result     TEXT,
    payload    TEXT  -- optional JSON blob for diff/log content
);

CREATE INDEX idx_replay_session ON replay_events(session_id, timestamp);

CREATE TABLE session_cache (
    session_id    TEXT PRIMARY KEY,
    workspace_id  TEXT NOT NULL,
    edge_id       TEXT NOT NULL,
    hub_meta_json TEXT NOT NULL,       -- serialized SessionHubMeta
    state         TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);
```

### 3.3 Risk Scoring

Risk scoring runs on the hub when it receives session state updates from edges. The hub has access to context percentage, test status, pending decision ages, and session state — all required inputs.

```rust
fn compute_risk(session: &SessionRecord, meta: &SessionHubMeta,
                pending: &[Decision]) -> RiskLevel {
    let oldest_pending_age = pending.iter()
        .filter(|d| d.resolved_at.is_none())
        .map(|d| Utc::now() - d.created_at)
        .max();

    // Red conditions
    if meta.context_pct > 85
        || session.state == SessionState::Errored
        || matches!(oldest_pending_age, Some(age) if age > Duration::minutes(15))
    {
        return RiskLevel::Red;
    }

    // Yellow conditions
    if meta.context_pct >= 70
        || meta.tests_status == TestsStatus::Failing
        || matches!(oldest_pending_age, Some(age) if age > Duration::minutes(5))
    {
        return RiskLevel::Yellow;
    }

    RiskLevel::Green
}
```

### 3.4 Attention Budget

The budget counter lives in the `attention_budget_log` table. To compute "used today":

```rust
fn budget_used_today(workspace: &Workspace, db: &Connection) -> u32 {
    let tz: Tz = workspace.attention_budget.reset_tz.parse().unwrap();
    let today_start = Utc::now().with_timezone(&tz).date().and_hms(0, 0, 0);
    let today_start_utc = today_start.with_timezone(&Utc);

    db.query_row(
        "SELECT COUNT(*) FROM attention_budget_log
         WHERE workspace_id = ?1 AND logged_at >= ?2",
        params![workspace.id, today_start_utc.to_rfc3339()],
        |row| row.get(0),
    ).unwrap_or(0)
}
```

---

## 4. Hub API

### 4.1 REST Endpoints

All endpoints are scoped by workspace via query param `?workspace_id=` or path prefix. Auth via `Authorization: Bearer <token>` header (existing mechanism).

#### Workspace

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/workspaces` | List workspaces for the authenticated org |
| `GET` | `/workspaces/:id` | Get workspace detail including budget state |

**`GET /workspaces/:id` response:**

```json
{
  "id": "ws-checkout",
  "org_id": "org-silverpond",
  "name": "Checkout Experience",
  "repos": [
    { "id": "payment-gateway", "label": "payment-gateway", "icon": "◈", "color": "#34D399" }
  ],
  "members": ["jono", "rowan", "alex", "sam"],
  "attention_budget": { "used": 7, "total": 12, "reset_tz": "Australia/Melbourne" }
}
```

#### Sessions

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/sessions?workspace_id=` | List sessions with hub metadata |
| `POST` | `/sessions` | Start session (proxy to edge) |
| `POST` | `/sessions/:id/stop` | Stop session (proxy to edge) |
| `GET` | `/sessions/:id/logs` | Proxy transcript from edge |
| `GET` | `/sessions/:id/usage` | Proxy usage from edge |

**`GET /sessions` response:**

```json
{
  "sessions": [
    {
      "id": "S-9593",
      "state": "Running",
      "edge_id": "edge-01",
      "agent": "claude",
      "model": "sonnet-4-5",
      "hub_meta": {
        "workspace_id": "ws-checkout",
        "goal": "Add Stripe webhook signature verification",
        "risk": "green",
        "context_pct": 58,
        "touched_repos": ["payment-gateway"],
        "pending_decisions": 2,
        "tests_status": "passing",
        "tokens_total": "52.7k",
        "estimated_cost_usd": 0.34,
        "lines_added": 312,
        "lines_removed": 41,
        "commits": 4
      },
      "created_at": "2026-02-27T08:00:00Z",
      "updated_at": "2026-02-27T09:23:00Z"
    }
  ],
  "stats": {
    "active": 3,
    "blocked": 1,
    "queued": 2,
    "complete": 1,
    "cost_today_usd": 5.83
  }
}
```

#### Decisions

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/decisions?workspace_id=&repo_id=&status=pending` | List decisions |
| `GET` | `/decisions/:id` | Decision detail with context |
| `POST` | `/decisions/:id/approve` | Approve decision |
| `POST` | `/decisions/:id/deny` | Deny decision |
| `POST` | `/decisions/:id/comment` | Add comment |

**`POST /decisions/:id/approve` request:**

```json
{
  "reviewer": "jono",
  "comment": "Looks good, proceed with env vars."
}
```

**`POST /decisions/:id/approve` response:**

```json
{
  "decision_id": "D-0041",
  "action": "approve",
  "session_unblocked": true
}
```

Decision actions trigger:
1. Write `DecisionResolution` to the `decisions` table.
2. Insert row into `attention_budget_log`.
3. Forward the resolution to the edge daemon for the owning session via `POST /sessions/:id/decision-response` on the edge API.
4. Broadcast a `decision_resolved` WebSocket event.

#### Replay

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/sessions/:id/replay/timeline` | Ordered replay events |
| `GET` | `/sessions/:id/replay/diff` | File changes grouped by repo |
| `GET` | `/sessions/:id/replay/terminal` | Raw terminal output (proxied from edge logs) |

**`GET /sessions/:id/replay/diff` response:**

```json
{
  "groups": [
    {
      "repo": "shared-protobufs",
      "files": [
        { "path": "proto/payment/v1/intent.proto", "additions": 12, "deletions": 0 }
      ]
    }
  ]
}
```

### 4.2 Edge API Additions

Forged needs two new endpoints to support decision flow:

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/sessions/:id/decision` | Agent emits a decision request (called by sidecar or agent hook) |
| `POST` | `/sessions/:id/decision-response` | Hub forwards reviewer action to unblock agent |

**Decision emission flow:**

```
Agent (Claude Code)
  ↓  hook/sidecar detects decision prompt
forged sidecar
  ↓  POST /sessions/:id/decision  (local edge endpoint)
forged
  ↓  POST /decisions  (to hub, with session + workspace context)
forgehub
  ↓  persists decision, broadcasts WS event
Dashboard
  ↓  reviewer clicks Approve
  ↓  POST /decisions/:id/approve
forgehub
  ↓  POST /sessions/:id/decision-response  (to edge)
forged
  ↓  injects response into tmux session
Agent continues
```

### 4.3 WebSocket Channels

The hub exposes two WebSocket endpoints. Both send JSON-framed messages.

#### `/sessions/ws?workspace_id=`

Pushes session list updates to connected dashboards.

**Message types:**

```json
{ "type": "session_update", "session": { /* full session object */ } }
{ "type": "session_removed", "session_id": "S-xxxx" }
{ "type": "stats_update", "stats": { "active": 3, "blocked": 1, ... } }
{ "type": "budget_update", "budget": { "used": 8, "total": 12 } }
```

**Push frequency:** On state change from edge heartbeat, or every 3 seconds if no changes (heartbeat keepalive).

#### `/decisions/ws?workspace_id=`

Pushes decision queue updates.

```json
{ "type": "decision_created", "decision": { /* full decision object */ } }
{ "type": "decision_resolved", "decision_id": "D-0041", "action": "approve" }
```

#### `/sessions/:id/attach`

Existing PTY relay WebSocket — unchanged. Uses the reliable stream protocol (RESUME/EVENT/INPUT/ACK/SNAPSHOT) already defined in `forged/src/stream.rs`.

---

## 5. Hub Backend Implementation

### 5.1 New Crate Dependencies

Add to `forgehub/Cargo.toml`:

```toml
sqlx = { version = "0.7", features = ["runtime-tokio", "sqlite", "postgres", "chrono"] }
chrono-tz = "0.9"
```

`sqlx` provides async DB access across SQLite and Postgres. `chrono-tz` handles
timezone-aware budget resets.

### 5.2 Hub Service Extensions

```rust
// forgehub/src/lib.rs additions

pub struct HubService {
    // ... existing fields ...
    db: sqlx::SqlitePool,
    decision_tx: broadcast::Sender<DecisionEvent>,   // WS broadcast
    session_tx: broadcast::Sender<SessionEvent>,     // WS broadcast
}

pub enum DecisionEvent {
    Created(Decision),
    Resolved { decision_id: String, action: DecisionAction },
}

pub enum SessionEvent {
    Updated(SessionWithMeta),
    Removed(String),
    StatsChanged(WorkspaceStats),
    BudgetChanged(AttentionBudget),
}
```

### 5.3 Edge Polling Loop

A background tokio task polls each registered edge every 3 seconds:

```rust
async fn poll_edges(hub: Arc<HubService>) {
    let mut interval = tokio::time::interval(Duration::from_secs(3));
    loop {
        interval.tick().await;
        let edges = hub.registry.lock().unwrap().clone();
        for (id, edge) in &edges {
            match reqwest::get(format!("{}/sessions", edge.addr)).await {
                Ok(resp) => {
                    let sessions: Vec<SessionRecord> = resp.json().await?;
                    hub.update_session_cache(id, sessions).await;
                }
                Err(e) => tracing::warn!(edge = %id, err = %e, "edge poll failed"),
            }
        }
    }
}
```

On each poll cycle, the hub:
1. Merges edge sessions into `session_cache`.
2. Recomputes risk scores.
3. Broadcasts `SessionEvent::Updated` for any changes.
4. Recomputes workspace stats and broadcasts if changed.

### 5.4 Decision Lifecycle

```rust
impl HubService {
    /// Called when edge reports a new decision from an agent.
    pub async fn create_decision(&self, mut decision: Decision) -> Result<Decision> {
        decision.id = format!("D-{:04}", self.next_decision_seq());
        decision.created_at = Utc::now();

        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO decisions (id, session_id, workspace_id, repo_id,
             question, context_json, severity, tags_json, impact_repo_ids,
             assigned_to, agent_goal, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![
                decision.id, decision.session_id, decision.workspace_id,
                decision.repo_id, decision.question,
                serde_json::to_string(&decision.context)?,
                serde_json::to_string(&decision.severity)?,
                serde_json::to_string(&decision.tags)?,
                serde_json::to_string(&decision.impact_repo_ids)?,
                decision.assigned_to, decision.agent_goal,
                decision.created_at.to_rfc3339(),
            ],
        )?;

        let _ = self.decision_tx.send(DecisionEvent::Created(decision.clone()));
        Ok(decision)
    }

    /// Called when a reviewer approves/denies/comments.
    pub async fn resolve_decision(
        &self, decision_id: &str, action: DecisionAction,
        reviewer: &str, comment: Option<String>,
    ) -> Result<()> {
        let now = Utc::now();
        let resolution = DecisionResolution {
            action: action.clone(),
            reviewer: reviewer.to_string(),
            comment,
            resolved_at: now,
        };

        let db = self.db.lock().unwrap();
        db.execute(
            "UPDATE decisions SET resolved_at = ?1, resolution_json = ?2
             WHERE id = ?3",
            params![
                now.to_rfc3339(),
                serde_json::to_string(&resolution)?,
                decision_id,
            ],
        )?;

        // Log for attention budget
        let decision: Decision = self.get_decision(decision_id)?;
        db.execute(
            "INSERT INTO attention_budget_log
             (workspace_id, decision_id, reviewer, action, logged_at)
             VALUES (?1,?2,?3,?4,?5)",
            params![
                decision.workspace_id, decision_id,
                reviewer, serde_json::to_string(&action)?,
                now.to_rfc3339(),
            ],
        )?;

        // Forward to edge to unblock the agent
        self.forward_decision_to_edge(&decision, &resolution).await?;

        // Broadcast
        let _ = self.decision_tx.send(
            DecisionEvent::Resolved { decision_id: decision_id.to_string(), action }
        );

        Ok(())
    }
}
```

### 5.5 Axum Route Registration

```rust
// forgehub/src/main.rs — add to existing router

let app = Router::new()
    // ... existing routes ...
    // Workspace
    .route("/workspaces", get(list_workspaces))
    .route("/workspaces/:id", get(get_workspace))
    // Decisions
    .route("/decisions", get(list_decisions))
    .route("/decisions/:id", get(get_decision))
    .route("/decisions/:id/approve", post(approve_decision))
    .route("/decisions/:id/deny", post(deny_decision))
    .route("/decisions/:id/comment", post(comment_decision))
    .route("/decisions/ws", get(decisions_ws))
    // Replay
    .route("/sessions/:id/replay/timeline", get(replay_timeline))
    .route("/sessions/:id/replay/diff", get(replay_diff))
    .route("/sessions/:id/replay/terminal", get(replay_terminal))
    .with_state(hub_service);
```

---

## 6. Frontend Implementation

### 6.1 Technology Choice

**Framework:** Preact + HTM (no build step required).

Rationale:
- The hub already serves static files from `dashboard/`. A build-step-free SPA keeps deployment simple (single binary embeds the HTML/JS).
- Preact is 3KB, API-compatible with React. HTM provides JSX-like tagged template syntax without transpilation.
- The JSX mockup uses inline styles and functional components — this maps directly to Preact.
- If the frontend grows beyond v1 complexity, migration to a Vite + React build is straightforward since the component structure is compatible.

**File structure:**

```
dashboard/
├── index.html              # shell, loads app.js via <script type="module">
├── app.js                  # main entry, router, top nav
├── lib/
│   ├── preact.module.js    # vendored preact (3KB)
│   ├── htm.module.js       # vendored htm
│   └── hooks.module.js     # vendored preact/hooks
├── components/
│   ├── shared.js           # RepoPill, Badge, Dot, Card, MiniBar, SectionLabel
│   ├── fleet.js            # FleetDashboard
│   ├── decisions.js        # DecisionQueue
│   ├── replay.js           # SessionReplay
│   └── nav.js              # TopNav, breadcrumb, live indicator
├── services/
│   ├── api.js              # REST client (fetch wrappers)
│   └── ws.js               # WebSocket manager (auto-reconnect)
├── theme.js                # T token object (colors, fonts)
└── state.js                # shared reactive state (signals or context)
```

### 6.2 Theme Tokens

Extracted from the JSX mockup as a JS module:

```js
// dashboard/theme.js
export const T = {
  bg0: "#0A0A0C", bg1: "#101013", bg2: "#161619",
  bg3: "#1E1E23", bg4: "#26262D",
  border: "#28282F", borderS: "#1C1C22",
  t1: "#E4E2DF", t2: "#98968F", t3: "#5A5955", t4: "#3A3937",
  ember: "#E8622C", emberG: "#F0884D",
  emberS: "rgba(232,98,44,0.08)",
  molten: "#FF9F43",
  ok: "#34D399", okD: "#22C55E",
  okS: "rgba(52,211,153,0.08)",
  okSolid: "rgba(52,211,153,0.15)",
  warn: "#FBBF24", warnS: "rgba(251,191,36,0.08)",
  err: "#F87171", errD: "#EF4444",
  errS: "rgba(248,113,113,0.08)",
  info: "#60A5FA", infoS: "rgba(96,165,250,0.08)",
  purple: "#A78BFA", purpleS: "rgba(167,139,250,0.08)",
  cyan: "#22D3EE",
  mono: "'JetBrains Mono', monospace",
  sans: "'Poppins', sans-serif",
  data: "'Outfit', sans-serif",
};
```

### 6.3 Shared Components

Directly ported from the JSX mockup. Each component is a Preact functional component using inline styles:

| Component | Props | Usage |
|-----------|-------|-------|
| `Dot` | `color, size, pulse` | Risk indicator, status dot, live dot |
| `Badge` | `children, color, bg` | Severity, status, test status, tags |
| `RepoPill` | `repoId, repos` | Repo identity everywhere |
| `MiniBar` | `value, max, color, h` | Context health bar |
| `SectionLabel` | `children` | Section headers (mono, uppercase) |
| `Card` | `children, style` | Container card with border |

### 6.4 View Components

#### FleetDashboard (`fleet.js`)

**Data source:** `GET /sessions?workspace_id=` + `/sessions/ws` WebSocket.

**Subcomponents:**
- `AttentionBudgetMeter` — conic-gradient ring, remaining count, reset note.
- `StatCard` — single metric card (count or dollar amount).
- `SessionRow` — risk-bordered card with goal, status badge, repo pills, context bar, test badge, token/cost stats.
- `QueuedPanel` / `CompletedPanel` — compact list with goal, repos, model or line counts.

**Behaviour:**
- On mount: fetch sessions, subscribe to `/sessions/ws`.
- On WS `session_update`: merge into local state, re-sort by risk.
- On WS `budget_update`: update meter.
- Red-risk sessions: CSS animation on the indicator dot (`@keyframes pulse`).

#### DecisionQueue (`decisions.js`)

**Data source:** `GET /decisions?workspace_id=&status=pending` + `/decisions/ws` WebSocket.

**Subcomponents:**
- `RepoFilterBar` — "All repos" + per-repo buttons. Client-side filter on `repo_id`.
- `DecisionCard` — severity stripe, severity dot, repo pill, impact chain, tags, question, agent info, age, reviewer.
- `DiffBlock` — syntax-highlighted diff lines with add/del coloring.
- `LogBlock` — monospace log output.
- `ScreenshotBlock` — italic description placeholder.

**Behaviour:**
- Clicking card body toggles expanded context (accordion, max one open).
- Approve/Deny/Comment buttons call REST API, on success card animates out (CSS transition: `max-height` + `opacity`).
- Sorting: critical > high > medium > low, then oldest first within each severity.
- On WS `decision_created`: prepend to list in correct sort position.
- On WS `decision_resolved`: animate card removal.

#### SessionReplay (`replay.js`)

**Data source:** `GET /sessions/:id/replay/timeline`, `GET /sessions/:id/replay/diff`, `GET /sessions/:id/logs`.

**Subcomponents:**
- `TimelineSidebar` — vertical timeline with event markers, repo-colored connector lines, context switch markers.
- `DiffView` — files grouped under repo headers with icon/color. File rows show path + add/del counts.
- `FileTreeView` — tree of repos > files, highlighted modified files.
- `StructuredLogView` — table with alternating rows, event type icons, repo labels, result badges.
- `TerminalView` — monospace dark background, color-coded lines.
- `AtomicMergeButton` — gradient CTA in tab bar (disabled in v1, shows PR count).

**Behaviour:**
- Tab switching between Unified Diff / File Tree / Structured Log / Terminal.
- Timeline click scrolls main pane to corresponding change.
- Session header in sidebar shows status dot, session ID, model badge, goal, repo pills.

### 6.5 WebSocket Manager

```js
// dashboard/services/ws.js
export function connectWS(path, { onMessage, onStatus }) {
  const protocol = location.protocol === "https:" ? "wss:" : "ws:";
  let ws, reconnectTimer;

  function connect() {
    onStatus?.("connecting");
    ws = new WebSocket(`${protocol}//${location.host}${path}`);
    ws.onopen = () => onStatus?.("live");
    ws.onclose = () => {
      onStatus?.("reconnecting");
      reconnectTimer = setTimeout(connect, 3000);
    };
    ws.onmessage = (e) => {
      try { onMessage(JSON.parse(e.data)); }
      catch (err) { console.warn("ws parse error", err); }
    };
  }

  connect();
  return {
    close: () => { clearTimeout(reconnectTimer); ws?.close(); },
  };
}
```

The live indicator in the top nav (`Dot` + "live" / "reconnecting" / "disconnected") is driven by the `onStatus` callback.

### 6.6 API Client

```js
// dashboard/services/api.js
const BASE = "";  // same origin

export async function fetchJSON(path, opts = {}) {
  const token = localStorage.getItem("forgemux_token");
  const headers = { "Content-Type": "application/json" };
  if (token) headers["Authorization"] = `Bearer ${token}`;
  const resp = await fetch(`${BASE}${path}`, { ...opts, headers });
  if (!resp.ok) throw new Error(`${resp.status} ${resp.statusText}`);
  return resp.json();
}

export const api = {
  sessions:    (wsId) => fetchJSON(`/sessions?workspace_id=${wsId}`),
  decisions:   (wsId) => fetchJSON(`/decisions?workspace_id=${wsId}&status=pending`),
  approve:     (id, body) => fetchJSON(`/decisions/${id}/approve`, { method: "POST", body: JSON.stringify(body) }),
  deny:        (id, body) => fetchJSON(`/decisions/${id}/deny`, { method: "POST", body: JSON.stringify(body) }),
  comment:     (id, body) => fetchJSON(`/decisions/${id}/comment`, { method: "POST", body: JSON.stringify(body) }),
  workspace:   (id) => fetchJSON(`/workspaces/${id}`),
  replayTimeline: (sid) => fetchJSON(`/sessions/${sid}/replay/timeline`),
  replayDiff:     (sid) => fetchJSON(`/sessions/${sid}/replay/diff`),
  replayLogs:     (sid) => fetchJSON(`/sessions/${sid}/logs`),
};
```

---

## 7. Decision Flow — Edge Integration

The agent-to-decision pipeline requires a mechanism on the edge for detecting when an agent has paused and is asking a question. Two approaches, used together:

### 7.1 Claude Code Hook (Preferred)

Claude Code supports user-defined hooks that fire on specific events. A `pre_tool_use` or `notification` hook can detect when the agent emits a question-type output and POST it to forged.

Configuration in the session's Claude Code settings:

```json
{
  "hooks": {
    "notification": [
      {
        "matcher": { "type": "tool_use", "tool": "AskHumanQuestion" },
        "command": "curl -s -X POST http://localhost:${FORGED_PORT}/sessions/${SESSION_ID}/decision -d @-"
      }
    ]
  }
}
```

This is the cleanest path but depends on the agent using a recognizable tool/pattern for questions.

### 7.2 Sidecar Prompt Detection (Fallback)

The existing `StateDetector` already watches for prompt patterns in terminal output. Extend it to detect decision-like prompts (e.g., lines ending with `?` after a multi-line explanation, or specific patterns like `[Y/n]`, `approve/deny`).

When detected, forged creates a decision record with the surrounding context (captured from the PTY ring buffer) and pushes it to the hub.

### 7.3 Edge Decision Endpoints

```rust
// forged/src/server.rs additions

/// Agent (via hook) reports a decision request.
async fn create_decision(
    State(svc): State<Arc<SessionService>>,
    Path(session_id): Path<String>,
    Json(payload): Json<CreateDecisionPayload>,
) -> impl IntoResponse {
    // Build Decision from payload + session metadata
    // Forward to hub: POST /decisions
}

/// Hub sends reviewer response back to edge.
async fn decision_response(
    State(svc): State<Arc<SessionService>>,
    Path(session_id): Path<String>,
    Json(payload): Json<DecisionResponsePayload>,
) -> impl IntoResponse {
    // Inject response into tmux session
    // e.g., "Approved: proceed with env vars approach"
    svc.inject_input(&session_id, &format_decision_response(&payload))?;
}
```

---

## 8. Replay Data Pipeline

### 8.1 Structured Event Emission (Edge)

Forged emits replay events as structured JSONL alongside the raw transcript. Each tool call, file edit, test run, and context switch is logged:

```rust
// forged/src/lib.rs addition

fn emit_replay_event(&self, session_id: &str, event: ReplayEvent) {
    let path = self.config.data_dir
        .join("sessions").join(session_id).join("replay.jsonl");
    let mut f = OpenOptions::new().create(true).append(true).open(&path)?;
    writeln!(f, "{}", serde_json::to_string(&event)?)?;
}
```

### 8.2 Hub Ingestion

On session completion (or periodically for active sessions), the hub pulls replay JSONL from the edge and stores it in `replay_events`:

```
GET /sessions/:id/replay.jsonl  (new edge endpoint, returns raw JSONL)
```

### 8.3 Diff Computation

The hub computes grouped diffs by:
1. Pulling the session's git worktree state from the edge (`GET /sessions/:id/diff`).
2. Running `git diff --stat` equivalent on each touched repo.
3. Grouping by repo with addition/deletion counts.

For v1, this is proxied from the edge. The edge runs the actual git commands since it has filesystem access.

---

## 9. Atomic Merge (Future — Deferred)

Documented here for completeness; not implemented in v1.

The "Atomic Merge -> N PRs" button will:
1. Call `POST /sessions/:id/atomic-merge` on the hub.
2. Hub iterates `touched_repos`, creates one branch + PR per repo via GitHub/GitLab API.
3. PRs share a `merge_group_id` in their description for cross-linking.
4. Hub stores merge group state for tracking.

Prerequisite: GitHub/GitLab integration credentials in workspace config.

---

## 10. Implementation Sequence

### Phase 1: Hub Storage + Decision API

1. Add `sqlx` dependency. Create DB init function with schema from section 3.2.
2. Implement `Organization` and `Workspace` types in `forgemux-core`.
3. Add `GET /workspaces`, `GET /workspaces/:id` endpoints.
4. Implement `Decision` type and CRUD in hub service.
5. Add decision REST endpoints (`GET /decisions`, `POST /.../approve|deny|comment`).
6. Add `attention_budget_log` tracking.
7. Add `/decisions/ws` WebSocket broadcast.

### Phase 2: Session Enrichment + Risk Scoring

1. Add `SessionHubMeta`, `RiskLevel`, `TestsStatus` types.
2. Implement edge polling loop with session cache merge.
3. Add `compute_risk()` function on hub side.
4. Extend `/sessions/ws` payload with risk, budget, stats.
5. Add `session_cache` table writes.

### Phase 3: Frontend — Fleet Dashboard

1. Set up `dashboard/` directory with Preact + HTM.
2. Port theme tokens, shared components from JSX.
3. Build `FleetDashboard` view (budget meter, stat cards, session rows).
4. Wire to `GET /sessions` and `/sessions/ws`.
5. Implement live indicator.

### Phase 4: Frontend — Decision Queue

1. Build `DecisionQueue` view (filter bar, cards, actions).
2. Wire to `GET /decisions` and `/decisions/ws`.
3. Implement expand/collapse, approve/deny/comment actions.
4. Implement card animation on resolve.

### Phase 5: Edge Decision Flow

1. Add `/sessions/:id/decision` and `/sessions/:id/decision-response` to forged.
2. Implement Claude Code hook for decision detection.
3. Implement sidecar fallback detection.
4. Wire edge -> hub -> dashboard -> hub -> edge round-trip.

### Phase 6: Frontend — Session Replay

1. Add replay endpoints to hub (proxy from edge).
2. Build `SessionReplay` view (timeline, diff, log, terminal tabs).
3. Wire to replay APIs.
4. Add edge `replay.jsonl` emission.

---

## 11. Open Questions

1. **Decision assignment.** Should decisions auto-assign to the workspace member who owns the primary repo, or go unassigned to the queue? The mockup shows `assigned_to` but some decisions are `null`. **Recommendation:** unassigned by default; workspace settings can configure per-repo default reviewers.

2. **Context percentage source.** Where does `context_pct` come from? Claude Code doesn't expose this directly. **Options:** (a) parse from agent JSONL usage logs if available, (b) estimate from `tokens_total / model_context_limit * 100`, (c) detect from agent output patterns (e.g., "context window 71% used"). **Recommendation:** option (b) as initial heuristic; refine with (c) when patterns are identified.

3. **Replay event granularity.** Should replay events be emitted for every tool call, or only for file modifications and test runs? Fine-grained events are useful but generate significant data. **Recommendation:** emit all events but only store in hub for completed sessions; active session replay proxied from edge ring buffer.

4. **Frontend framework migration.** If the SPA grows significantly beyond v1, should we migrate from Preact + HTM to a build-step framework (Vite + React/Solid)? **Recommendation:** defer until the dashboard exceeds ~10 views or needs code splitting. The inline-style approach from the mockup keeps component count manageable.

5. **Multi-workspace WebSocket multiplexing.** Should a user watching two workspaces open two WebSocket connections, or should a single connection support subscribing to multiple workspace IDs? **Recommendation:** one connection per workspace for simplicity in v1. Multiplexing adds protocol complexity with minimal benefit since users typically focus on one workspace.
