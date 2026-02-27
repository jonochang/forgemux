# Research: Superset Deep Dive & Forgemux Ideas (Round 2)

**Date:** February 27, 2026
**Scope:** Deep technical comparison after full source review of both codebases.
**Prior art:** `superset.md` covered high-level positioning and 10 initial ideas.
This document goes deeper into architecture, implementation patterns, and
concrete technical borrowings.

---

## 1. Superset Architecture in Detail

### Monorepo Structure (Turborepo + Bun)

Superset is a large TypeScript monorepo with 5 apps and 11+ shared packages:

| Layer | Packages |
|-------|----------|
| **Apps** | `desktop` (Electron), `web` (Next.js dashboard), `api` (Next.js backend), `admin`, `marketing` |
| **Core** | `db` (Postgres/Drizzle), `local-db` (SQLite/Drizzle), `auth` (Better Auth), `trpc` (tRPC router) |
| **UI** | `ui` (Radix + Tailwind component library) |
| **Agent** | `chat` (slash commands), `chat-mastra` (Mastracode runtime bridge), `mcp` (MCP server), `desktop-mcp` (stdio MCP) |
| **Infra** | `shared` (constants, types), `email` (React Email templates) |

Key takeaway: Superset invests heavily in **typed boundaries** between
packages. tRPC gives end-to-end type safety from database schema to React
component, with no OpenAPI spec or code-gen step.

### Dual Database Strategy

- **PostgreSQL (Neon)** for cloud state: users, orgs, tasks, device presence,
  integrations, subscriptions.
- **SQLite (better-sqlite3)** in the Electron desktop app for offline-capable
  local state: workspaces, local tasks, settings.
- **Electric SQL** bridges the two by streaming Postgres changes to clients in
  real-time over HTTP/2 (proxied through Caddy to avoid browser connection
  limits).

### Agent Runtime

Superset doesn't wrap agents in tmux. Instead:

1. **node-pty** spawns real PTY processes inside the Electron app.
2. **xterm.js** renders the terminal in the renderer process.
3. Agent wrapper scripts in `~/.superset[-{workspace}]/bin/` prepend PATH and
   inject env vars before delegating to the real agent binary.
4. **Mastracode** (a forked agent runtime from `superset-sh/mastra`) handles
   tool execution, hooks, and MCP integration for the built-in chat agent.

### Multi-Device Coordination

- `device_presence` table tracks which devices are online per user.
- `agent_commands` table stores commands targeted at specific devices with
  timeout, result, and error payloads (JSONB).
- This enables a web dashboard to send commands to a specific desktop instance
  (e.g., "start a new workspace on my laptop").

### Authentication & Billing

- Better Auth with OAuth (Google, GitHub), API keys, org-based RBAC.
- Stripe integration for subscription tiers and billing.
- Upstash Redis for rate limiting.
- Resend for transactional email.

---

## 2. Architectural Comparison

| Dimension | Superset | Forgemux |
|-----------|----------|----------|
| **Language** | TypeScript (full stack) | Rust |
| **Session substrate** | node-pty (in-process PTY) | tmux (external process manager) |
| **Durability** | App-level state in SQLite + Postgres | File-based JSON SessionStore |
| **Real-time sync** | Electric SQL over HTTP/2 | WebSocket + event ring |
| **API style** | tRPC (typed RPC) | REST + WebSocket |
| **Desktop** | Electron app with tray, auto-update | CLI-only (fmux) |
| **Browser attach** | xterm.js in Electron renderer | WebSocket PTY bridge |
| **Agent abstraction** | Wrapper scripts + env injection | AgentAdapter trait + config |
| **Multi-node** | Device presence + command routing | Edge registry + heartbeat |
| **Auth** | Better Auth + OAuth + RBAC | Optional bearer tokens |
| **Observability** | PostHog analytics, Sentry errors | Prometheus metrics, transcripts |
| **State model** | Implicit (UI-driven) | Explicit 7-state FSM with OCC |
| **Billing** | Stripe subscriptions | None |
| **Code size** | ~100K+ lines (estimate) | ~7K lines |

### Where Superset is stronger

1. **Product surface area.** Desktop app, web dashboard, admin panel, marketing
   site, email templates -- full SaaS stack.
2. **Integration ecosystem.** Linear, GitHub, Slack, Stripe out of the box.
3. **Type-safe API contracts.** tRPC removes entire classes of integration bugs.
4. **Real-time sync.** Electric SQL provides Postgres-to-client streaming with
   minimal client-side code.
5. **Multi-device UX.** Device presence + command routing lets users interact
   across machines.
6. **MCP support.** Both server-side and desktop-side MCP servers expose tools
   to agents.

### Where Forgemux is stronger

1. **Session durability.** tmux sessions survive daemon restarts, SSH drops,
   machine reboots. node-pty processes die with the Electron app.
2. **Explicit state machine.** 7-state FSM with optimistic concurrency gives
   precise lifecycle guarantees. Superset's session state is implicit.
3. **Headless operation.** Forgemux runs on any Linux box with tmux -- no GUI,
   no Electron, no browser required.
4. **Edge/hub topology.** First-class multi-node architecture with edge
   registration, heartbeats, and session aggregation.
5. **Resource constraints.** Session-level CPU/memory/PID/network policy
   placeholders (Superset doesn't isolate at this level).
6. **Foreman supervisor.** Built-in concept of an oversight agent that monitors
   other agents -- not present in Superset.
7. **Lightweight footprint.** ~7K lines of Rust vs. a large JS monorepo.
   Deploys as 3 static binaries.

---

## 3. Ideas for Forgemux (Deeper Cut)

### 3.1 Steal: Electric SQL-style Real-Time Sync

**What Superset does:** Uses Electric SQL to stream Postgres WAL changes to
browser clients over HTTP/2. Clients get a live-updating view of all sessions,
tasks, and device state without polling.

**Forgemux translation:**
- The hub already aggregates session state from edges. Add a Server-Sent Events
  (SSE) or HTTP/2 streaming endpoint that pushes session record changes to
  dashboard clients.
- SSE is simpler than WebSocket for one-way data push and avoids connection
  limits with HTTP/2 multiplexing.
- This replaces the current dashboard's need to poll `/api/sessions`.

**Effort:** Medium. Add an SSE endpoint to forgehub that fans out session state
changes. Dashboard JS subscribes on load.

### 3.2 Steal: Device Presence & Command Routing

**What Superset does:** Tracks which devices are online, lets web UI route
commands to specific devices.

**Forgemux translation:**
- Edges already register with the hub and send heartbeats. Extend this to
  expose a "device" concept in the dashboard: show which edges are online, their
  load, and let users choose which edge to start a session on.
- Add a command queue: hub accepts "start session" commands from the dashboard,
  routes them to the selected edge, and streams back the result.
- This turns the hub dashboard from read-only monitoring into an active control
  plane.

**Effort:** Medium. Edge registry already exists; add command dispatch and a
small UI form.

### 3.3 Steal: MCP Server for Tool Exposure

**What Superset does:** Runs MCP servers (both in the API and in the desktop
app) that expose tools to agents. The desktop MCP server uses stdio transport
and can automate browser actions via Puppeteer.

**Forgemux translation:**
- Add an MCP server to forged that exposes session management tools:
  `list_sessions`, `start_session`, `stop_session`, `get_transcript`,
  `get_session_status`.
- Agents running inside forgemux sessions could then use MCP to interact with
  forgemux itself -- e.g., a Foreman agent could call `list_sessions` via MCP
  instead of parsing CLI output.
- Use stdio transport (simplest) with a `forgemux-mcp` binary that connects to
  the local forged API.

**Effort:** Medium. The Rust MCP ecosystem (rmcp, mcp-rs) is maturing. A thin
stdio wrapper over the existing REST API would suffice.

### 3.4 Steal: Workspace Presets with Setup Scripts

**What Superset does:** Workspace presets define startup scripts, environment
setup, and dependency installation that run when a workspace is created.

**Forgemux translation:**
- Add a `[presets]` section to `forgemux.toml`:
  ```toml
  [presets.rust-project]
  setup = ["cargo check", "cargo test --no-run"]
  env = { RUST_LOG = "debug" }
  agent = "claude"
  model = "opus"

  [presets.node-project]
  setup = ["npm install", "npm run build"]
  env = { NODE_ENV = "development" }
  ```
- `fmux start --preset rust-project` applies the preset before launching the
  agent.
- Presets could also live in the repo (`.forgemux/preset.toml`) for
  project-specific defaults.

**Effort:** Low. Config parsing + shell command execution before agent launch.

### 3.5 Steal: Integration Webhooks (GitHub, Linear, Slack)

**What Superset does:** Has webhook handlers for GitHub events (PR opened,
review requested) and Stripe billing events. Linear and Slack integrations for
task sync and notifications.

**Forgemux translation:**
- Start with **outbound webhooks** from forgehub: POST session state changes to
  a configurable URL. This enables Slack bots, CI pipelines, or custom dashboards
  to react to agent events.
  ```toml
  [[webhooks]]
  url = "https://hooks.slack.com/services/..."
  events = ["session.waiting_input", "session.errored", "session.terminated"]
  secret = "hmac-secret"
  ```
- Then add **inbound GitHub webhooks**: when a PR is opened or an issue is
  assigned, forgehub can auto-create a session on an edge to work on it.
- This is the path toward "event-driven agent orchestration" which neither
  project fully implements yet.

**Effort:** Low for outbound (HTTP POST on state change). Medium for inbound
(webhook verification, event parsing, session creation logic).

### 3.6 Steal: tRPC-style Typed API Contracts

**What Superset does:** Uses tRPC for end-to-end type safety between backend
and frontend.

**Forgemux translation:**
- Forgemux is Rust, so tRPC doesn't apply directly. But the principle does:
  generate TypeScript types from Rust structs for the dashboard.
- Use `ts-rs` or `specta` to derive TypeScript interfaces from
  `SessionRecord`, `SessionState`, `StreamEvent`, etc.
- Auto-generate a `types.ts` file during build that the dashboard JS imports.
- This prevents the dashboard from drifting out of sync with the API.

**Effort:** Low. Add `#[derive(TS)]` to core structs, generate types in build
script.

### 3.7 Adapt: Local SQLite for Edge State

**What Superset does:** Uses SQLite in the desktop app for offline-capable
local persistence (Drizzle ORM, schema migrations).

**Forgemux translation:**
- Forgemux currently uses file-based JSON for session state. This works but
  has limitations:
  - No indexed queries (listing by state, agent, repo requires scanning all
    files).
  - No atomic multi-record updates.
  - No structured transcript storage.
- Migrate `SessionStore` to SQLite (via `rusqlite` or `sqlx`):
  ```sql
  CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    state TEXT NOT NULL,
    agent TEXT,
    model TEXT,
    repo TEXT,
    version INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    metadata JSON
  );
  CREATE INDEX idx_sessions_state ON sessions(state);
  ```
- Keep the `SessionStore` trait so file-based and SQLite backends coexist.
- Benefits: faster queries, proper indexing, WAL mode for concurrent reads,
  structured transcript/event storage.

**Effort:** Medium. Schema design + migration from JSON files. The trait
abstraction already exists.

### 3.8 Adapt: Organization & Multi-Tenant Model

**What Superset does:** Full org-based multi-tenancy with roles (owner, admin,
member), invitations, and per-org settings.

**Forgemux translation:**
- Forgemux doesn't need full SaaS multi-tenancy, but a lightweight
  "workspace" or "project" concept would help:
  - Group sessions by project/team.
  - Apply different policies per project.
  - Scope dashboard views.
- Add a `project` field to `SessionRecord` (optional, defaults to repo name).
- Hub dashboard filters by project.
- Policy configs can be per-project in `forgemux.toml`.

**Effort:** Low. One new field + config section + dashboard filter.

### 3.9 New Idea: Agent Protocol Abstraction (Beyond Wrappers)

**Observation:** Superset uses wrapper scripts + env vars to integrate agents.
Forgemux uses `AgentAdapter` with prompt patterns. Both are fragile -- they
depend on agent CLI output format staying stable.

**Proposal:** Define a lightweight **Forgemux Agent Protocol** that agents can
opt into:
- Agent writes structured JSON events to a well-known file descriptor or
  Unix socket: `{"event": "waiting_input", "ts": "..."}`,
  `{"event": "tool_call", "tool": "bash", "ts": "..."}`.
- forged reads this alongside PTY output. Structured signals take precedence
  over prompt pattern matching.
- For agents that don't support the protocol, fall back to the current
  `StateDetector` behavior.
- This is more robust than prompt matching and more portable than wrapper
  scripts.

**Implementation:**
- forged creates a Unix socket at a known path
  (`$FORGEMUX_EVENT_SOCKET`).
- Agent adapter checks for socket events in addition to PTY output.
- Claude Code's hook system could write to this socket with minimal config.

**Effort:** Medium. Unix socket listener + event schema + fallback logic.

### 3.10 New Idea: Session Templates from Superset's Task Model

**Observation:** Superset has a `tasks` table that tracks work items with
metadata (branch, description, status). Tasks drive workspace creation.

**Proposal:** Add **session templates** to forgemux:
```toml
[[templates]]
name = "fix-issue"
agent = "claude"
model = "opus"
worktree = true
prompt = "Fix the issue described in: {issue_url}"
notifications = ["waiting_input", "errored"]

[[templates]]
name = "review-pr"
agent = "claude"
model = "sonnet"
worktree = false
prompt = "Review the PR at: {pr_url}"
```

Usage: `fmux start --template fix-issue --var issue_url=https://github.com/...`

This bridges the gap between "start a raw agent session" and "start a
task-oriented workflow" without building a full task management system.

**Effort:** Low-medium. Template config + variable substitution + integration
with existing start flow.

### 3.11 New Idea: Dashboard as Separate Deployable

**Observation:** Superset has separate `web` and `api` apps. Forgemux embeds
a minimal HTML dashboard in the hub binary.

**Proposal:** Extract the dashboard into a separate lightweight app:
- A small Vite/React SPA that talks to forgehub's REST API.
- Auto-generated TypeScript types from Rust structs (idea 3.6).
- SSE subscription for live updates (idea 3.1).
- Session list with filters, transcript viewer, diff viewer, controls.
- Ship as a static bundle embedded in forgehub (current approach) but also
  allow standalone deployment for customization.

This keeps the hub binary self-contained for simple setups while allowing
teams to customize or extend the dashboard.

**Effort:** Medium-high. Separate frontend project, but can start from the
existing embedded HTML.

---

## 4. Priority Matrix

| Idea | Impact | Effort | Priority |
|------|--------|--------|----------|
| 3.4 Workspace presets | High | Low | **P0** |
| 3.8 Project grouping | Medium | Low | **P0** |
| 3.6 Generated TS types | Medium | Low | **P1** |
| 3.1 SSE live sync | High | Medium | **P1** |
| 3.5 Outbound webhooks | High | Low | **P1** |
| 3.9 Agent protocol | High | Medium | **P1** |
| 3.7 SQLite session store | High | Medium | **P2** |
| 3.3 MCP server | Medium | Medium | **P2** |
| 3.2 Command routing | Medium | Medium | **P2** |
| 3.10 Session templates | Medium | Low-Med | **P2** |
| 3.5 Inbound GitHub hooks | High | Medium | **P3** |
| 3.11 Dashboard extraction | Medium | Med-High | **P3** |

---

## 5. Key Strategic Takeaways

1. **Don't compete on desktop UX.** Superset's Electron app is their moat.
   Forgemux's moat is headless durability, edge topology, and Rust's deployment
   simplicity. Double down on server-side strengths.

2. **The integration layer matters more than the session layer.** Both projects
   can run agents in terminals. The differentiator is how sessions connect to
   the rest of the developer workflow: GitHub, CI, Slack, issue trackers. This
   is where forgemux should invest next.

3. **Structured agent communication is the next frontier.** Both projects
   currently rely on PTY output parsing to detect agent state. Defining a
   lightweight event protocol (3.9) would be a genuine innovation that could
   become a community standard.

4. **SQLite is the right persistence upgrade.** File-based JSON is fine for
   prototyping but won't scale. SQLite with WAL mode gives concurrent reads,
   indexed queries, and atomic writes with minimal operational overhead -- no
   external database to run.

5. **Type-safe dashboard integration is low-hanging fruit.** Auto-generating
   TypeScript from Rust structs (ts-rs/specta) eliminates a class of bugs as
   the dashboard grows. Do this before the dashboard gets more complex.

6. **Event-driven session creation is the path to automation.** Today, sessions
   are created manually via `fmux start`. Adding webhook-triggered session
   creation (GitHub issue assigned → agent session starts) is the step change
   that makes forgemux an orchestration platform rather than a session manager.
