# Forgemux Hub — Technology Design Specification

**Version:** 1.0
**Date:** 2026-02-27
**Status:** Final Draft
**Supersedes:** `tech_design.md`, `tech_design2.md`, `tech_design3.md`
**Source inputs:** `initial.md`, `forgemux-hub-stories.md`, `forgemux-hub.jsx`

---

## 1. Executive Summary

This document specifies the architecture, data model, API surface, frontend design, and implementation plan for the Forgemux Hub Dashboard — a web-based Agent Observability and Intervention Center for managing AI coding agents at scale.

The dashboard solves three core problems identified in competitive analysis:

1. **Cognitive overload** — raw terminal output replaced by structured, multi-modal session views
2. **Notification fatigue** — uncontrolled interrupts replaced by attention budgeting and risk scoring
3. **Lack of trust** — black-box agents replaced by visual timelines, diffs, and cross-repo impact visibility

**Key Technical Decisions:**

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Frontend framework | Preact + HTM (no build step) | Hub serves static files from embedded binary; 3KB runtime; JSX mockup maps directly; React migration path preserved |
| Real-time transport | WebSocket | Already implemented for attach relay; single protocol for all real-time; bidirectional if needed |
| Backend DB layer | `sqlx` (SQLite dev / Postgres prod) | Async, compile-time checked SQL, lightweight, matches axum + Tokio stack |
| Styling | Inline styles (CSS-in-JS) | No build step, dynamic theming, matches mockup approach |
| Risk scoring location | Hub | Hub has all required inputs (context %, test status, pending decision age, session state) |

**Non-goals for v1:**

- PM tooling (tickets, Kanban, sprint planners)
- Light theme
- Atomic Merge (deferred to v2)
- Agent config UIs (temperature, model selection)
- Mobile responsive layout

---

## 2. System Architecture

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

**Key constraints:**

- The SPA never talks directly to edge nodes. All traffic goes through the hub.
- Forged remains the source of truth for session lifecycle, logs, and PTY streams.
- The hub owns decision state, attention budgets, workspace metadata, and risk scoring.
- Auth is token-based (`Authorization: Bearer <token>`) via the existing pairing flow.

### 2.1 Communication Patterns

| Pattern | Transport | Direction | Use |
|---------|-----------|-----------|-----|
| Initial data load | REST (HTTP) | Request/Response | Sessions, decisions, workspace, replay data |
| Session state updates | WebSocket `/sessions/ws` | Server → Client | Risk changes, stats, budget ticks |
| Decision updates | WebSocket `/decisions/ws` | Server → Client | New decisions, resolutions |
| Decision actions | REST (HTTP POST) | Client → Server | Approve, deny, comment |
| Terminal attach | WebSocket `/sessions/:id/attach` | Bidirectional | PTY relay (existing RESUME/EVENT/INPUT/ACK protocol) |

---

## 3. Data Model

### 3.1 Entity Hierarchy

```
Organization (tenant)
  └── Workspace (team domain, bundles repos)
        ├── WorkspaceRepo[] (repos with icon + color identity)
        ├── Session[] (AI agent tasks, sourced from edges)
        │     └── ReplayEvent[] (timeline of agent actions)
        ├── Decision[] (pause points needing human input)
        └── AttentionBudget (daily decision cap)
```

### 3.2 Rust Types (forgemux-core)

All types are `Serialize + Deserialize + Clone`.

```rust
// --- Organization & Workspace ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Organization {
    pub id: String,
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
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DecisionContext {
    Diff { file: String, lines: Vec<DiffLine> },
    Log { text: String },
    Screenshot { description: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffLine {
    pub line_type: DiffLineType,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffLineType { Ctx, Add, Del }

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Severity { Critical = 0, High = 1, Medium = 2, Low = 3 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    pub id: String,
    pub session_id: String,
    pub workspace_id: String,
    pub repo_id: String,
    pub question: String,
    pub context: DecisionContext,
    pub severity: Severity,
    pub tags: Vec<String>,
    pub impact_repo_ids: Vec<String>,
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

// --- Session Hub Metadata ---

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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel { Green, Yellow, Red }

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TestsStatus { Passing, Failing, Pending, None }

// --- Replay Event ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayEvent {
    pub id: u64,
    pub session_id: String,
    pub repo_id: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub elapsed: String,
    pub event_type: ReplayEventType,
    pub action: String,
    pub result: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayEventType {
    System, Read, Edit, Tool, Switch, Test, Decision,
}
```

### 3.3 SQL Schema (Hub Storage)

Hub storage: `{hub_data_dir}/hub.db` (SQLite for dev/test), Postgres for production. Same schema and queries via `sqlx`.

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
    repos_json            TEXT NOT NULL DEFAULT '[]',
    members_json          TEXT NOT NULL DEFAULT '[]'
);

CREATE TABLE decisions (
    id                TEXT PRIMARY KEY,
    session_id        TEXT NOT NULL,
    workspace_id      TEXT NOT NULL REFERENCES workspaces(id),
    repo_id           TEXT NOT NULL,
    question          TEXT NOT NULL,
    context_json      TEXT NOT NULL,
    severity          TEXT NOT NULL,
    tags_json         TEXT NOT NULL DEFAULT '[]',
    impact_repo_ids   TEXT NOT NULL DEFAULT '[]',
    assigned_to       TEXT,
    agent_goal        TEXT NOT NULL,
    created_at        TEXT NOT NULL,
    resolved_at       TEXT,
    resolution_json   TEXT
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
    payload    TEXT
);

CREATE INDEX idx_replay_session ON replay_events(session_id, timestamp);

CREATE TABLE session_cache (
    session_id    TEXT PRIMARY KEY,
    workspace_id  TEXT NOT NULL,
    edge_id       TEXT NOT NULL,
    hub_meta_json TEXT NOT NULL,
    state         TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);
```

---

## 4. Hub API

All endpoints are workspace-scoped via query param or path. Auth via `Authorization: Bearer <token>` header (existing pairing mechanism).

### 4.1 REST Endpoints

#### Workspace

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/workspaces` | List workspaces for authenticated org |
| `GET` | `/workspaces/:id` | Workspace detail including budget state |

Response example (`GET /workspaces/:id`):

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
| `GET` | `/sessions?workspace_id=` | List sessions with hub metadata and stats |
| `POST` | `/sessions` | Start session (proxy to edge) |
| `POST` | `/sessions/:id/stop` | Stop session (proxy to edge) |
| `GET` | `/sessions/:id/logs` | Proxy transcript from edge |
| `GET` | `/sessions/:id/usage` | Proxy usage from edge |

Response example (`GET /sessions`):

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

Request/response (`POST /decisions/:id/approve`):

```json
// Request
{ "reviewer": "jono", "comment": "Looks good, proceed with env vars." }

// Response
{ "decision_id": "D-0041", "action": "approve", "session_unblocked": true }
```

Decision action side effects:
1. Write `DecisionResolution` to `decisions` table
2. Insert row into `attention_budget_log`
3. Forward resolution to edge via `POST /sessions/:id/decision-response`
4. Broadcast `decision_resolved` WebSocket event

#### Replay

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/sessions/:id/replay/timeline` | Ordered replay events |
| `GET` | `/sessions/:id/replay/diff` | File changes grouped by repo |
| `GET` | `/sessions/:id/replay/terminal` | Raw terminal output (proxied from edge) |

Response example (`GET /sessions/:id/replay/diff`):

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

Two new endpoints on forged for decision flow:

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/sessions/:id/decision` | Agent emits decision request (via hook/sidecar) |
| `POST` | `/sessions/:id/decision-response` | Hub forwards reviewer action to unblock agent |

### 4.3 WebSocket Channels

#### `/sessions/ws?workspace_id=`

Pushes session list updates. Message types:

```json
{ "type": "session_update", "session": { /* full session object */ } }
{ "type": "session_removed", "session_id": "S-xxxx" }
{ "type": "stats_update", "stats": { "active": 3, "blocked": 1, ... } }
{ "type": "budget_update", "budget": { "used": 8, "total": 12 } }
```

Push frequency: on state change, or every 3 seconds as heartbeat keepalive.

#### `/decisions/ws?workspace_id=`

Pushes decision queue updates:

```json
{ "type": "decision_created", "decision": { /* full decision object */ } }
{ "type": "decision_resolved", "decision_id": "D-0041", "action": "approve" }
```

#### `/sessions/:id/attach`

Existing PTY relay — unchanged. Uses RESUME/EVENT/INPUT/ACK/SNAPSHOT protocol.

---

## 5. Hub Backend Implementation

### 5.1 Crate Dependencies

Add to `forgehub/Cargo.toml`:

```toml
sqlx = { version = "0.7", features = ["runtime-tokio", "sqlite", "postgres", "chrono"] }
chrono-tz = "0.9"
```

### 5.2 Hub Service Extensions

```rust
pub struct HubService {
    // ... existing fields ...
    db: sqlx::SqlitePool,
    decision_tx: broadcast::Sender<DecisionEvent>,
    session_tx: broadcast::Sender<SessionEvent>,
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
1. Merges edge sessions into `session_cache`
2. Recomputes risk scores
3. Broadcasts `SessionEvent::Updated` for any changes
4. Recomputes workspace stats and broadcasts if changed

### 5.4 Risk Scoring

Risk scoring runs on the hub when it receives session state updates from edges.

```rust
fn compute_risk(
    session: &SessionRecord,
    meta: &SessionHubMeta,
    pending: &[Decision],
) -> RiskLevel {
    let oldest_pending_age = pending.iter()
        .filter(|d| d.resolved_at.is_none())
        .map(|d| Utc::now() - d.created_at)
        .max();

    // Red conditions (any triggers red)
    if meta.context_pct > 85
        || session.state == SessionState::Errored
        || matches!(oldest_pending_age, Some(age) if age > Duration::minutes(15))
    {
        return RiskLevel::Red;
    }

    // Yellow conditions (any triggers yellow)
    if meta.context_pct >= 70
        || meta.tests_status == TestsStatus::Failing
        || matches!(oldest_pending_age, Some(age) if age > Duration::minutes(5))
    {
        return RiskLevel::Yellow;
    }

    RiskLevel::Green
}
```

Risk thresholds summary:

| Level | Context % | Tests | Pending Decision Age | Other |
|-------|-----------|-------|---------------------|-------|
| Green | < 70% | Passing | < 5m | — |
| Yellow | 70–85% | Failing | 5–15m | — |
| Red | > 85% | — | > 15m | Errored state, looping (>3 consecutive failed tool calls) |

### 5.5 Attention Budget

Budget counter lives in `attention_budget_log`. To compute "used today":

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

Budget total is configurable per workspace. Resets at midnight in the workspace's configured timezone.

### 5.6 Decision Lifecycle

```rust
impl HubService {
    pub async fn create_decision(&self, mut decision: Decision) -> Result<Decision> {
        decision.id = format!("D-{:04}", self.next_decision_seq());
        decision.created_at = Utc::now();
        // Insert into decisions table
        // Broadcast DecisionEvent::Created
        Ok(decision)
    }

    pub async fn resolve_decision(
        &self, decision_id: &str, action: DecisionAction,
        reviewer: &str, comment: Option<String>,
    ) -> Result<()> {
        let now = Utc::now();
        // Update decisions table with resolution
        // Insert into attention_budget_log
        // Forward to edge: POST /sessions/:id/decision-response
        // Broadcast DecisionEvent::Resolved
        Ok(())
    }
}
```

### 5.7 Axum Route Registration

```rust
let app = Router::new()
    // ... existing routes ...
    // Workspace
    .route("/workspaces", get(list_workspaces))
    .route("/workspaces/:id", get(get_workspace))
    // Sessions (extend existing)
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

## 6. Decision Flow — Edge Integration

### 6.1 Full Round-Trip

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

### 6.2 Detection Methods

**Claude Code Hook (preferred):**

A `notification` hook detects when the agent emits a question-type tool call and POSTs it to forged:

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

**Sidecar prompt detection (fallback):**

The existing `StateDetector` watches for prompt patterns in terminal output (lines ending with `?`, `[Y/n]`, `approve/deny` patterns). On detection, forged creates a decision with surrounding context from the PTY ring buffer.

---

## 7. Replay Data Pipeline

### 7.1 Structured Event Emission (Edge)

Forged emits replay events as structured JSONL alongside the raw transcript:

```rust
fn emit_replay_event(&self, session_id: &str, event: ReplayEvent) {
    let path = self.config.data_dir
        .join("sessions").join(session_id).join("replay.jsonl");
    let mut f = OpenOptions::new().create(true).append(true).open(&path)?;
    writeln!(f, "{}", serde_json::to_string(&event)?)?;
}
```

### 7.2 Hub Ingestion

On session completion (or periodically for active sessions), the hub pulls replay JSONL from the edge:

```
GET /sessions/:id/replay.jsonl  (edge endpoint, returns raw JSONL)
```

### 7.3 Diff Computation

For v1, diffs are proxied from the edge. The edge runs `git diff --stat` equivalent on each touched repo since it has filesystem access. The hub groups results by repo with addition/deletion counts.

---

## 8. Frontend Implementation

### 8.1 Technology Choice

**Framework:** Preact + HTM (no build step required).

Rationale:
- The hub already serves static files from `dashboard/`. No-build-step SPA keeps deployment simple (single binary embeds the HTML/JS/CSS).
- Preact is 3KB, API-compatible with React. HTM provides JSX-like tagged template syntax without transpilation.
- The JSX mockup (`forgemux-hub.jsx`) uses inline styles and functional components — maps directly to Preact.
- If the frontend grows beyond ~10 views, migration to Vite + React is straightforward since the component structure is compatible.

### 8.2 File Structure

```
dashboard/
├── index.html              # shell, loads app.js via <script type="module">
├── app.js                  # main entry, router, top nav
├── lib/
│   ├── preact.module.js    # vendored preact (3KB)
│   ├── htm.module.js       # vendored htm
│   └── hooks.module.js     # vendored preact/hooks
├── components/
│   ├── shared.js           # Dot, Badge, RepoPill, MiniBar, Card, SectionLabel
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

### 8.3 Design System

#### Theme Tokens

Extracted from the JSX mockup:

```js
export const T = {
  // Backgrounds (5 depth levels)
  bg0: "#0A0A0C", bg1: "#101013", bg2: "#161619",
  bg3: "#1E1E23", bg4: "#26262D",

  // Borders
  border: "#28282F", borderS: "#1C1C22",

  // Text
  t1: "#E4E2DF", t2: "#98968F", t3: "#5A5955", t4: "#3A3937",

  // Semantic colors
  ember: "#E8622C", emberG: "#F0884D", emberS: "rgba(232,98,44,0.08)",
  molten: "#FF9F43",
  ok: "#34D399", okD: "#22C55E", okS: "rgba(52,211,153,0.08)", okSolid: "rgba(52,211,153,0.15)",
  warn: "#FBBF24", warnS: "rgba(251,191,36,0.08)",
  err: "#F87171", errD: "#EF4444", errS: "rgba(248,113,113,0.08)",
  info: "#60A5FA", infoS: "rgba(96,165,250,0.08)",
  purple: "#A78BFA", purpleS: "rgba(167,139,250,0.08)",
  cyan: "#22D3EE", cyanS: "rgba(34,211,238,0.08)",

  // Typography
  mono: "'JetBrains Mono', monospace",
  sans: "'Poppins', sans-serif",
  data: "'Outfit', sans-serif",
};
```

#### Color Semantics

| Token | Hex | Meaning |
|-------|-----|---------|
| `ember` | `#E8622C` | Primary brand, CTAs, active tab |
| `ok` | `#34D399` | Success, active, tests passing |
| `warn` | `#FBBF24` | Warning, idle, attention needed |
| `err` | `#F87171` | Error, blocked, tests failing |
| `info` | `#60A5FA` | Informational, complete |
| `purple` | `#A78BFA` | Merged, Opus model indicator |
| `molten` | `#FF9F43` | High severity, cost values |

#### Three-Font Typography System

| Token | Font | Weights | Usage |
|-------|------|---------|-------|
| `T.sans` | Poppins | 300–500 | Headers (500), body text (400), labels, buttons, nav |
| `T.data` | Outfit | 300–500 | Stat numbers (300), costs, token counts, percentages, diffs |
| `T.mono` | JetBrains Mono | 400–600 | Session IDs, branch names, file paths, terminal, code |

Rules:
- Numbers/metrics humans scan → `T.data` (Outfit)
- Words humans read → `T.sans` (Poppins)
- Code, identifiers, paths → `T.mono` (JetBrains Mono)
- Max weight 500 for Poppins/Outfit; only `T.mono` uses 600

### 8.4 Component Hierarchy

```
ForgemuxHub (App Shell)
├── TopNavigation
│   ├── Logo
│   ├── Breadcrumb (Org / Workspace)
│   ├── NavTabs (Dashboard | Decisions | Session Replay)
│   ├── RepoIndicators
│   └── LiveIndicator
│
├── FleetDashboard
│   ├── AttentionBudgetMeter
│   ├── SessionSummaryStats (5 stat cards)
│   ├── SessionRiskHeatmap
│   │   └── SessionRow (risk-colored border, goal, repos, metrics)
│   ├── QueuedPanel
│   └── CompletedPanel
│
├── DecisionQueue
│   ├── RepoFilterBar
│   └── DecisionCard
│       ├── SeverityStripe
│       ├── RepoPills + ImpactChain
│       ├── QuickActions (Approve / Deny / Comment)
│       └── ExpandedContext (DiffBlock / LogBlock / ScreenshotBlock)
│
└── SessionReplay
    ├── TimelineSidebar
    │   └── TimelineEvent (repo-colored markers, context switches)
    ├── TabBar (Unified Diff | File Tree | Structured Log | Terminal)
    ├── UnifiedDiffView (files grouped by repo)
    ├── FileTreeView (repos > files, modified highlighted)
    ├── StructuredLogView (table with event icons, repo labels, result badges)
    ├── TerminalView (monospaced, color-coded raw output)
    └── AtomicMergeCTA (disabled in v1)
```

### 8.5 Shared Components

Ported directly from `forgemux-hub.jsx`:

| Component | Props | Usage |
|-----------|-------|-------|
| `Dot` | `color, size, pulse` | Risk indicator, status dot, live dot |
| `Badge` | `children, color, bg` | Severity, status, test status, tags |
| `RepoPill` | `repoId` | Repo identity (icon + color) everywhere |
| `MiniBar` | `value, max, color, h` | Context health bar |
| `SectionLabel` | `children` | Section headers (mono, uppercase, 600 weight) |
| `Card` | `children, style` | Container with border and bg1 background |

Helper functions:

```js
const riskColor = (r) => ({ green: T.ok, yellow: T.warn, red: T.err }[r]);
const statusColor = (s) => ({ active: T.ok, blocked: T.err, queued: T.t3, complete: T.info }[s]);
const contextColor = (v) => v < 40 ? T.ok : v < 70 ? T.molten : v < 85 ? T.warn : T.err;
const severityColor = (s) => ({ critical: T.errD, high: T.molten, medium: T.info, low: T.t3 }[s]);
```

### 8.6 View Behaviour

#### FleetDashboard

- **Data source:** `GET /sessions?workspace_id=` + `/sessions/ws` WebSocket
- On mount: fetch sessions, subscribe to WS
- On WS `session_update`: merge into local state, re-sort by risk
- On WS `budget_update`: update attention meter
- Red-risk sessions: CSS `@keyframes pulse` animation on indicator dot
- Stat cards show: Active, Blocked, Queued, Complete, Cost Today

#### DecisionQueue

- **Data source:** `GET /decisions?workspace_id=&status=pending` + `/decisions/ws` WebSocket
- Sorting: critical > high > medium > low, then oldest first within severity
- Client-side repo filtering (instant, no reload)
- Accordion expand: max one card open at a time
- On approve/deny/comment: REST call, on success card animates out (CSS `max-height` + `opacity` transition)
- On WS `decision_created`: insert in correct sort position
- On WS `decision_resolved`: animate removal

#### SessionReplay

- **Data source:** `GET /sessions/:id/replay/timeline`, `/replay/diff`, `/logs`
- Tab switching: Unified Diff / File Tree / Structured Log / Terminal
- Timeline click scrolls main pane to corresponding change
- Unified Diff: files grouped under repo headers with icon, color, file count
- Terminal: color-coded (ember for prompts, green for success, red for errors, purple for context switches)
- Atomic Merge button present but disabled in v1

### 8.7 WebSocket Manager

```js
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
  return { close: () => { clearTimeout(reconnectTimer); ws?.close(); } };
}
```

The live indicator in TopNav (`Dot` + "live" / "reconnecting" / "disconnected") is driven by `onStatus`.

### 8.8 API Client

```js
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
  workspace:      (id)       => fetchJSON(`/workspaces/${id}`),
  sessions:       (wsId)     => fetchJSON(`/sessions?workspace_id=${wsId}`),
  decisions:      (wsId)     => fetchJSON(`/decisions?workspace_id=${wsId}&status=pending`),
  approve:        (id, body) => fetchJSON(`/decisions/${id}/approve`, { method: "POST", body: JSON.stringify(body) }),
  deny:           (id, body) => fetchJSON(`/decisions/${id}/deny`, { method: "POST", body: JSON.stringify(body) }),
  comment:        (id, body) => fetchJSON(`/decisions/${id}/comment`, { method: "POST", body: JSON.stringify(body) }),
  replayTimeline: (sid)      => fetchJSON(`/sessions/${sid}/replay/timeline`),
  replayDiff:     (sid)      => fetchJSON(`/sessions/${sid}/replay/diff`),
  replayLogs:     (sid)      => fetchJSON(`/sessions/${sid}/logs`),
};
```

---

## 9. Security and Access Control

### 9.1 Authentication

- Token-based auth with existing pairing flow (`POST /pairing/start`, `POST /pairing/exchange`)
- `Authorization: Bearer <token>` required when auth is enabled
- Tokens map to workspace permissions

### 9.2 Authorization

- All endpoints enforce workspace-scoped access checks
- Decision resolve actions record `reviewer` identity and timestamp in the `resolution` and `attention_budget_log`
- Log access gated by workspace permissions

### 9.3 Frontend Security

- All user input escaped via Preact's default rendering (no `dangerouslySetInnerHTML`)
- Diff content rendered via component structure, not raw HTML injection
- Token stored in `localStorage` (acceptable for SPA; httpOnly cookies add complexity without meaningful gain for a bearer-token API)

### 9.4 Future (v2)

- Role-based access: viewer, member, admin
- Per-repo default reviewers
- Workspace membership management

---

## 10. Performance Considerations

### 10.1 Data Density Targets

- Tech Lead assesses fleet health in < 5 seconds
- Clear 5 decisions in < 60 seconds
- All counts, statuses, and risk scores update without manual refresh

### 10.2 Frontend Performance

- **Session lists > 50 items:** Virtualize scroll (simple windowing)
- **Timeline > 100 events:** Lazy render with intersection observer
- **Diff content:** Load full diff on expand (not pre-fetched)
- **Memoization:** Decision filtering is client-side; memoize computed filtered/sorted lists
- **Debouncing:** If search/filter inputs are added, debounce at 300ms

### 10.3 Backend Performance

- Edge polling: 3-second interval, parallel per-edge requests
- Session cache in SQLite avoids repeated edge queries for the same data
- WebSocket broadcast: `tokio::sync::broadcast` channel, minimal overhead
- Decision queries: indexed on `(workspace_id, resolved_at)` and `(severity, created_at)`

---

## 11. Non-Functional Requirements

- **No single-repo assumptions.** Every view must accommodate sessions touching 1–N repos.
- **Data density over chrome.** Optimize for scan speed.
- **Dark mode default.** Light mode is a non-goal for v1.
- **Real-time updates.** WebSocket preferred. All counts/statuses update without manual refresh.
- **Keyboard-first (future).** All primary actions reachable via keyboard shortcuts. Nav: `1/2/3`. Decisions: `a/d/c`. Escape to close expanded cards.

---

## 12. Implementation Sequence

### Phase 1: Hub Storage + Decision API

1. Add `sqlx` + `chrono-tz` dependencies to `forgehub/Cargo.toml`
2. Create DB init function with schema from section 3.3
3. Implement `Organization`, `Workspace`, `WorkspaceRepo` types in `forgemux-core`
4. Add `GET /workspaces`, `GET /workspaces/:id` endpoints
5. Implement `Decision` type and CRUD in hub service
6. Add decision REST endpoints (`GET /decisions`, `POST .../approve|deny|comment`)
7. Add `attention_budget_log` tracking
8. Add `/decisions/ws` WebSocket broadcast

### Phase 2: Session Enrichment + Risk Scoring

1. Add `SessionHubMeta`, `RiskLevel`, `TestsStatus` types
2. Implement edge polling loop with session cache merge
3. Add `compute_risk()` function on hub
4. Extend `/sessions/ws` payload with risk, budget, stats
5. Add `session_cache` table writes

### Phase 3: Frontend — Fleet Dashboard

1. Set up `dashboard/` directory with Preact + HTM (vendored modules)
2. Port theme tokens and shared components from JSX mockup
3. Build `FleetDashboard` (budget meter, stat cards, session risk heatmap, queued/completed panels)
4. Wire to `GET /sessions` and `/sessions/ws`
5. Implement live indicator in top nav

### Phase 4: Frontend — Decision Queue

1. Build `DecisionQueue` (filter bar, decision cards with severity stripe, quick actions)
2. Wire to `GET /decisions` and `/decisions/ws`
3. Implement expand/collapse context, approve/deny/comment actions
4. Card animation on resolve

### Phase 5: Edge Decision Flow

1. Add `/sessions/:id/decision` and `/sessions/:id/decision-response` to forged
2. Implement Claude Code hook for decision detection
3. Implement sidecar fallback detection in `StateDetector`
4. Wire full round-trip: edge → hub → dashboard → hub → edge

### Phase 6: Frontend — Session Replay

1. Add replay endpoints to hub (proxy from edge)
2. Build `SessionReplay` (timeline sidebar, unified diff, file tree, structured log, terminal tabs)
3. Wire to replay APIs
4. Add edge `replay.jsonl` emission

---

## 13. Open Questions and Recommendations

| # | Question | Recommendation |
|---|----------|----------------|
| 1 | **Decision assignment:** auto-assign to repo owner or unassigned? | Unassigned by default. Workspace settings can configure per-repo default reviewers in v2. |
| 2 | **Context percentage source:** how to get `context_pct`? | Estimate from `tokens_total / model_context_limit * 100` as initial heuristic. Refine with agent output pattern detection later. |
| 3 | **Replay event granularity:** emit all tool calls or only file modifications + tests? | Emit all events on edge, but only ingest into hub storage for completed sessions. Active session replay proxied from edge ring buffer. |
| 4 | **Frontend framework migration:** when to move from Preact+HTM to a build-step framework? | Defer until dashboard exceeds ~10 views or needs code splitting. |
| 5 | **Multi-workspace WebSocket:** one connection per workspace or multiplexed? | One connection per workspace for v1 simplicity. |
| 6 | **Replay events format from edge:** how should edge emit them? | Structured JSONL file per session (`replay.jsonl`), appended on each tool call. |

---

## Appendix A: Acceptance Criteria Summary

### Fleet Dashboard

- Attention budget: ring gauge with green → amber → red transition, remaining count, reset note
- Stat cards: Active, Blocked, Queued, Complete, Cost Today (real-time)
- Session rows: risk border + dot (pulsing for red), goal, status badge, repo pills, context bar %, test badge, tokens, cost, uptime
- Queued panel: goal, target repos, model
- Completed panel: goal, repos, lines +/-, cost, duration

### Decision Queue

- Repo filter bar: instant client-side filtering, persists within session
- Decision cards: severity stripe + dot (pulsing for critical), repo pill, impact chain, severity/tag badges, question, agent info, age, reviewer
- Quick actions: Approve/Deny/Comment always visible without expanding
- Expanded context: diff (syntax-highlighted with file path), log (monospaced), screenshot (italic description)
- Sorting: severity descending, then age ascending within severity
- Critical count called out separately in header

### Session Replay

- Timeline sidebar: vertical with event markers, repo-colored connectors, context switch markers (larger), decision events (red background)
- Event type icons: system `◦`, read `◎`, edit `✎`, tool `⚡`, switch `⇋`, test `▷`, decision `⬡`
- Unified diff: files grouped by repo header (icon, color, file count)
- File tree: only repos the session modified, modified files highlighted
- Structured log: table with timestamp, event icon, repo, action, result badge
- Terminal: monospaced dark, color-coded (ember prompts, green success, red errors, purple context switches)
- Atomic Merge CTA: gradient button showing PR count (disabled v1)

### Navigation

- Breadcrumb: `Organization / Workspace` with clickable workspace switcher
- Repo indicators: compact icon strip in top nav
- Decision badge: ember-colored count on Decisions tab
- Live indicator: green dot + "live" / amber "reconnecting" / red "disconnected"
