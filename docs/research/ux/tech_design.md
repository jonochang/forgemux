# Forgemux Hub Technology Design

**Source inputs:** `docs/research/ux/forgemux-hub-stories.md`, `docs/research/ux/forgemux-hub.jsx`  
**Date:** 2026-02-27  
**Target:** Hub + Dashboard (Enterprise Workspace Edition)

## 1. Scope and Goals

**Primary goal:** deliver a multi-tenant hub + dashboard that supports:
- Workspace fleet view (risk, cost, attention budget)
- Decision queue with fast approve/deny/comment
- Multi-repo session replay
- Real-time updates and attach

**Non-goals for v1:**
- PM tooling (tickets/kanban)
- Light theme
- Automated PR creation (Atomic Merge is a later phase)

## 2. High-Level Architecture

```
Browser SPA  <----WS/HTTP---->  forgehub (hub API)
                                    |
                                    |  HTTP/WS relay
                                    v
                                 forged (edge daemon)
                                    |
                                    v
                                 tmux + repo worktrees
```

**Frontend:** single-page app embedded in forgehub (static HTML/JS/CSS).  
**Backend:** forgehub provides REST + WS APIs, delegates session operations to edge nodes (forged), and stores hub metadata.  
**Edge:** forged remains the source of truth for session state, logs, usage, and attach streaming.

## 3. Core Data Model

**Organization**
- `id`, `name`

**Workspace**
- `id`, `org_id`, `name`, `timezone`
- `attention_budget_total` (daily)
- `repos[]` (see Repo)

**Repo**
- `id`, `workspace_id`, `name`, `icon`, `color`

**Session**
- `id`, `workspace_id`, `edge_id`, `agent`, `model`
- `goal`, `state`, `risk`, `context_pct`
- `repo_roots[]`, `touched_repos[]`
- `tokens_prompt`, `tokens_completion`, `tokens_total`, `estimated_cost_usd`
- `pending_decisions`, `tests_status`, `uptime`, `last_activity_at`
- `version` (optimistic concurrency)

**Decision**
- `id`, `session_id`, `workspace_id`, `repo_id`
- `question`, `severity`, `tags[]`, `impact_repo_ids[]`
- `assigned_to`, `created_at`, `resolved_at`
- `context` (diff/log/screenshot reference)

**Replay Event**
- `id`, `session_id`, `repo_id`, `ts`, `type`, `payload`
- For timeline, diff grouping, log table, and terminal stream

**Usage**
- `session_id`, `prompt_tokens`, `completion_tokens`, `total_tokens`
- `estimated_cost_usd`

## 4. Hub Services and Responsibilities

**4.1 Session Aggregation**
- Pull sessions from edges (HTTP polling or edge push).
- Maintain a lightweight cache keyed by `(workspace_id, session_id)` for dashboard and filters.
- Surface risk, attention budget usage, and decision counts in real-time.

**4.2 Decision Queue**
- Persist decisions in hub storage with repo/impact metadata.
- Support filtering by repo and severity sorting.
- Provide approve/deny/comment APIs that forward to edge and record audit info.

**4.3 Replay Data**
- Proxy logs/usage from edge for now (v1).
- Long-term: ingest structured replay events into hub storage for richer timeline and multi-repo diffs.

**4.4 Auth + Tokens**
- Token-based access per workspace (pairing flow already implemented).
- `Authorization: Bearer <token>` required when enabled.
- Session scope: tokens map to workspace permissions.

## 5. API Design

### 5.1 REST Endpoints (Hub)

**Workspace / Sessions**
- `GET /sessions` -> list sessions for workspace
- `POST /sessions` -> start session (proxy to edge)
- `POST /sessions/:id/stop` -> stop session
- `GET /sessions/:id/logs` -> proxy to edge logs
- `GET /sessions/:id/usage` -> proxy to edge usage

**Decision Queue**
- `GET /decisions` -> list decisions (filter by repo, severity)
- `POST /decisions/:id/approve`
- `POST /decisions/:id/deny`
- `POST /decisions/:id/comment`

**Replay**
- `GET /sessions/:id/replay/timeline`
- `GET /sessions/:id/replay/diff`
- `GET /sessions/:id/replay/logs`

**Pairing / Tokens**
- `POST /pairing/start`
- `POST /pairing/exchange`
- `GET /pair` (landing UI)

### 5.2 WebSockets

**`/sessions/ws`**
- Push session list updates (current behavior)
- Extend payload to include risk, budget, decision stats

**`/decisions/ws`**
- Push decision updates to enable real-time queue

**`/sessions/:id/attach`**
- Relay to edge attach socket (already implemented)

## 6. Frontend System Design

### 6.1 Views
**Fleet Dashboard**
- Attention budget meter
- Session stats strip
- Risk heatmap list
- Queued and completed panels

**Decision Queue**
- Repo filter bar
- Severity-sorted list
- Expandable context (diff/log/screenshot)
- Quick actions (approve/deny/comment)

**Session Replay**
- Timeline with repo switches and decisions
- Unified diff grouped by repo
- File tree (future)
- Structured log view
- Raw terminal view

### 6.2 Components (from JSX)
- `RepoPill`, `Badge`, `Dot`, `Card`, `MiniBar`
- Decision cards with severity stripe and actions
- Session rows with risk badge, context bar, test badge

### 6.3 Design System (from UX spec)
- Fonts: Poppins (sans), Outfit (data), JetBrains Mono (mono)
- Dark UI defaults
- Semantic colors: ember/ok/warn/err/info/molten/purple

## 7. Risk Scoring and Attention Budget

**Risk score logic (v1):**
- Green: context < 70%, tests passing, no pending decisions older than 15m
- Yellow: context 70-85% OR tests failing OR pending decision 5-15m old
- Red: context > 85% OR loops OR pending decisions > 15m OR blocked state

**Attention budget:**
- Daily decision cap per workspace
- Meter updates with decision resolve events
- Reset at workspace timezone midnight

## 8. Storage Strategy

**Phase 1 (immediate):**
- Hub stores decisions, workspace settings, repo metadata
- Sessions remain in edge JSON store
- Replay data proxied from edge

**Phase 2:**
- Hub stores session cache + replay events
- Edge continues to own source-of-truth for session lifecycle

**Suggested DB:** SQLite for dev, Postgres for production.  
Schema aligns with data model above.

**DB layer choice:** `sqlx`
- Async and aligns with `axum` + Tokio.
- Compile-time SQL checking for safer queries.
- Supports SQLite (dev/test) and Postgres (prod) with one query surface.
- Lightweight compared to a full ORM.

**Alternative:** `sea-orm` (only if we need heavier ORM ergonomics).

## 9. Security and Compliance

- Token-based auth with optional pairing flow
- Workspace-scoped access checks on all endpoints
- Audit fields on decisions: who approved/denied/commented and when
- Log access should be gated by workspace permissions

## 10. Release Plan (Incremental)

1. Extend sessions/ws payload (risk, stats, budget, decision counts)
2. Implement decisions REST + WS and build decision queue UI
3. Add replay endpoints (proxy from edge), wire timeline + diff (static first)
4. Persist decisions and replay events in hub storage
5. Add search/filter + keyboard shortcuts

## 11. Open Questions

- Where should risk scoring run? (edge vs hub)
- Should replay events be emitted from edge as structured JSONL?
- Do we add workspace membership and per-repo ACL now or later?
