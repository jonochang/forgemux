# Mission Control Deep Review (Source Code Analysis)

Date: February 27, 2026
Sources: Full source code review of /home/jonochang/lib/mission-control

Previous review (mission-control.md) was based on the README only. This review is based on reading the actual implementation: daemon system, data layer, validation schemas, security module, prompt builder, dispatcher, scheduler, runner, health monitor, and type definitions.

---

## Architecture Summary

Mission Control is a Next.js 15 app (TypeScript strict mode) that stores all state in local JSON files under `data/`. The web UI reads/writes through Next.js API routes. An optional background daemon (`scripts/daemon/`) polls for pending tasks and spawns `claude -p` processes to execute them.

```
┌──────────────────────┐
│  Next.js Web UI      │  React 19, Tailwind v4, shadcn/ui, dnd-kit
│  (localhost:3000)    │  Eisenhower matrix, Kanban, inbox, crew, skills
└──────────┬───────────┘
           │ API routes (GET/POST)
┌──────────▼───────────┐
│  Data Layer          │  JSON files in data/
│  (lib/data.ts)       │  Per-file Mutex for write safety
│  Zod validation      │  17 schemas, field-level errors
└──────────┬───────────┘
           │ reads data/ directly
┌──────────▼───────────┐
│  Daemon              │  Node.js background process
│  (scripts/daemon/)   │  Scheduler → Dispatcher → AgentRunner
│  Spawns claude -p    │  Health monitor, PID tracking, retry queue
└──────────────────────┘
```

Key property: **no network protocol between daemon and app**. They communicate through the filesystem. The daemon reads `tasks.json` to find work and writes `daemon-status.json` for the dashboard. The web UI reads the same files. Concurrency safety comes from per-file async mutexes in the Node.js process (but not across processes — the daemon reads files without locking).

---

## What the Source Code Reveals (Beyond the README)

### 1. Daemon Execution Model

The daemon is a well-structured Node.js process with four components:

- **Scheduler** (`scheduler.ts`): Wraps `node-cron`. Starts polling on `*/N * * * *` interval and scheduled commands (daily-plan at 7am, standup at 9am weekdays, weekly-review Friday 5pm). Supports hot-reload of config.

- **Dispatcher** (`dispatcher.ts`): Polls `tasks.json` for tasks with `kanban: "not-started"` and a non-null, non-"me" `assignedTo`. Sorts by Eisenhower priority (DO > SCHEDULE > DELEGATE > ELIMINATE). Filters out blocked tasks (dependency checking), tasks with pending decisions, already-running tasks, and tasks exceeding retry limits. Dispatches up to `maxParallelAgents` concurrently. Has a persistent retry queue with exponential backoff (base delay * 2^attempt, capped at 60 min).

- **AgentRunner** (`runner.ts`): Spawns `claude -p <prompt> --output-format json --max-turns N`. Cross-platform binary detection (Linux, macOS, Windows .cmd shim resolution). Tree-kill for timeout enforcement. Safe environment construction (strips all env vars except PATH, HOME, TEMP, and Windows system vars). Output captured with 10MB cap.

- **HealthMonitor** (`health.ts`): Tracks active sessions, history (last 50), cumulative stats. Writes status to `daemon-status.json` via atomic rename. Proactive stale session cleanup every 60s (checks PIDs via `kill(pid, 0)`). Persists stats across daemon restarts.

**Key insight**: Mission Control's daemon is essentially a task queue with cron scheduling bolted on. It doesn't manage terminal sessions — it spawns fire-and-forget CLI processes. The agent runs, exits, and the daemon reads the exit code. There's no PTY capture, no attach/detach, no live observation of what the agent is doing.

### 2. Security Model

The security module (`security.ts`) has three notable patterns:

- **Credential scrubbing**: 14 regex patterns covering API keys, bearer tokens, AWS keys, GitHub/npm/Slack/Stripe/Anthropic tokens, SSH private keys, database connection strings, and generic password/token patterns. Applied to all logged output and stored errors.

- **Prompt injection defense**: Task data is wrapped in `<task-context>` fences with escape of closing tags inside content. Prompts are capped at 100KB.

- **Binary whitelist**: Only `claude`, `claude.cmd`, `claude.exe` can be spawned. Combined with safe env construction that strips all env vars except system essentials.

- **Path traversal prevention**: `validatePathWithinWorkspace()` resolves and checks paths against the workspace root.

**Comparison with Forgemux**: Forgemux uses bearer tokens for API auth and optional AES-256-GCM stream encryption. Mission Control has no network auth (local-only), but has stronger output sanitization. Forgemux doesn't currently scrub credentials from transcripts or logs.

### 3. Data Model Depth

The Zod schemas reveal the full data model:

**Task** is the central entity:
- Eisenhower axes: `importance` (important/not-important) × `urgency` (urgent/not-urgent)
- Kanban: `not-started` → `in-progress` → `done`
- Assignment: `assignedTo` (single agent) + `collaborators` (additional agents)
- Subtasks with progress tracking (done/not-done)
- Acceptance criteria as string array
- Time tracking: estimated vs actual minutes
- Comments with author attribution
- Tags, notes, due dates, soft delete
- Blocking dependencies: `blockedBy` task IDs (max 50)

**Agent** is a first-class registry entry:
- `id`, `name`, `icon`, `description`
- `instructions` (up to 20KB of agent system prompt)
- `capabilities` (string array, up to 50)
- `skillIds` (links to skills library)
- `status` (active/inactive)

**Skill** is a reusable knowledge module:
- `content` (up to 50KB)
- `agentIds` (bidirectional: agents link to skills, skills link to agents)

**Inbox message**: typed (delegation, report, question, update, approval), with from/to agent routing, task linkage, read/unread/archived status.

**Decision**: question + options + context, requested by an agent, linked to a task. Blocks task execution until answered.

**Active runs**: tracks PID, status (running/completed/failed/timeout), exit code, error.

### 4. Prompt Construction

The prompt builder (`prompt-builder.ts`) assembles:
1. Agent persona (name, description, instructions, capabilities)
2. Linked skills content (resolved bidirectionally from both agent.skillIds and skill.agentIds)
3. Task instructions (title, ID, priority, description, subtasks, acceptance criteria, notes, estimate)
4. Standard Operating Procedures (read ai-context.md, check inbox, execute work, don't do bookkeeping)
5. Subtask progress tracking instructions (if task has subtasks)

The SOP is interesting: agents are explicitly told NOT to update task status or write to inbox/activity-log. The daemon's post-completion hook handles bookkeeping. This prevents agents from corrupting shared state.

### 5. Concurrency Safety

The data layer uses per-file `async-mutex` instances. Every write goes through `mutate*()` helpers that atomically read → callback → write within the lock. The `with*()` helpers are read-only locks (noted as "legacy — read-only inside lock" with deadlock warnings).

Limitation: This is in-process mutex only. The daemon reads files directly without going through the web API, so there's a race condition between the daemon and the web UI writing to the same files concurrently. In practice this works because the daemon mostly reads tasks.json and the UI mostly writes to it, but it's not airtight.

---

## Architectural Comparison With Forgemux

| Dimension | Mission Control | Forgemux |
|-----------|----------------|----------|
| **Execution substrate** | `child_process.spawn(claude -p)` | tmux sessions with PTY capture |
| **Session durability** | None — process dies, session gone | Full — survives network drops, reconnectable |
| **Observation** | Exit code + JSON output only | Live terminal stream, transcript, events |
| **State detection** | N/A (fire-and-forget) | Regex + JSONL log watching for Running/Idle/WaitingInput |
| **Multi-node** | Single machine only | Edge/hub separation, multi-node |
| **Human intervention** | Decisions queue (async) | WaitingInput state + WebSocket attach (real-time) |
| **Agent definition** | First-class: registry, instructions, skills, capabilities | Minimal: agent type (Claude/Codex), model, policy |
| **Task management** | Full: Eisenhower, Kanban, subtasks, dependencies, estimates | None (sessions are the unit) |
| **Communication** | Inbox (delegation, report, question, approval) | Notification hooks (desktop, webhook, command) |
| **Scheduling** | node-cron with daily-plan, standup, weekly-review | None built-in |
| **Retry** | Persistent queue with exponential backoff | None (session state machine handles errors) |
| **Credential handling** | 14-pattern scrubbing on all output | Creds stay on edge, optional stream encryption |
| **Write safety** | Per-file async-mutex | Optimistic concurrency (session versioning) |
| **Tech stack** | Next.js/TypeScript/Node | Rust/tokio/axum |
| **Storage** | JSON files (single machine) | JSON files (edge) + optional SQLite/Postgres (hub) |

**Core tension**: Mission Control treats agent sessions as opaque execution units (spawn, wait, read exit code). Forgemux treats sessions as observable, interactive, durable processes. These are complementary layers, not competing ones.

---

## Ideas for Forgemux (Deeper Than v1 Doc)

### A. Task-to-Session Bridge (Refined)

The first doc proposed a minimal task layer. Having now seen Mission Control's actual data model, here's a more specific design:

**Don't replicate Mission Control's task model in Forgemux.** Instead, define a `WorkItem` that is thinner than a task but richer than a bare session:

```rust
struct WorkItem {
    id: WorkItemId,           // "W-xxx"
    intent: String,           // What to accomplish
    repo_root: PathBuf,
    agent_spec: AgentSpec,    // Agent type + model + optional persona
    acceptance: Vec<String>,  // What "done" looks like
    sessions: Vec<SessionId>, // Spawned sessions (may be >1 if retried or split)
    outcome: Option<Outcome>, // Success/Failure/Partial + summary
    created_at: DateTime,
    completed_at: Option<DateTime>,
    source: Option<ExternalRef>, // e.g., { system: "mission-control", task_id: "abc123" }
}
```

The `source` field enables integration with Mission Control (or Linear, or GitHub Issues) without coupling to any one system. The `sessions` field provides the bridge: one work item may produce multiple sessions (first attempt + retry, or lead + helper).

**Hub API**: `POST /work-items` creates a work item and starts a session. `GET /work-items/:id` returns the work item with linked session states. `POST /work-items/:id/retry` spawns a new session. This keeps Forgemux infrastructure-first while adding just enough task awareness to be useful.

### B. Agent Persona + Skills Injection (Concrete)

Mission Control's prompt builder pattern is directly portable. Forgemux already has `SessionRole` (Worker/Foreman) and agent config. Extend with:

```toml
# forged.toml
[agent_personas.developer]
name = "Developer"
instructions = """
You are a senior developer. Follow the project's coding conventions.
Run tests before committing. Write clear commit messages.
"""
capabilities = ["code", "test", "refactor"]
skills_dir = "/etc/forgemux/skills/"  # Directory of .md files to inject

[agent_personas.researcher]
name = "Researcher"
instructions = "..."
```

When starting a session with `--persona developer`, forged prepends the persona's instructions + skills content to the agent's initial prompt. This is a config-driven feature that doesn't change the session model.

The skills directory pattern is better than Mission Control's JSON-embedded content because:
- Skills are plain markdown files that can be version-controlled independently
- No 50KB limit baked into a schema
- Can be shared across projects via symlinks or a skills repo

### C. Scheduled Session Launcher

Mission Control's scheduler (daily-plan, standup, weekly-review) mapped to Forgemux:

```toml
# forgehub.toml or forged.toml
[[schedules]]
name = "morning-review"
cron = "0 7 * * *"
action = "session"  # or "foreman-check"
agent = "claude"
model = "sonnet"
persona = "developer"
repo = "/home/user/project"
intent = "Run morning review: check for stale PRs, failed CI, and blocking issues"
```

Implementation: A scheduler component in forgehub (or forged for single-node) uses a cron library to trigger session starts. Each scheduled session gets a WorkItem with `source: { system: "schedule", name: "morning-review" }`. The foreman already provides periodic supervision — this just adds a way to trigger non-foreman sessions on a schedule.

### D. Post-Session Bookkeeping Protocol

Mission Control's SOP pattern (tell the agent NOT to do bookkeeping, let the system handle it) is a good idea. Forgemux could define a post-session hook:

```toml
[post_session]
on_success = "scripts/post-success.sh"  # Called with session ID, transcript path
on_failure = "scripts/post-failure.sh"
on_timeout = "scripts/post-timeout.sh"
```

The hook could:
- Parse the session's last output for a structured report
- POST the report to an inbox API (Mission Control's or Forgemux's own)
- Update a work item's outcome
- Trigger a retry (creating a new session)

This keeps Forgemux as infrastructure while enabling higher-level workflow integration.

### E. Credential Scrubbing

Mission Control's 14-pattern credential scrubber is worth adopting directly. Forgemux stores transcripts and event logs that may contain:
- API keys typed into terminals
- Database connection strings
- Bearer tokens in curl commands
- SSH keys accidentally displayed

Add a `scrub_credentials(text: &str) -> String` function to `forgemux-core` and apply it to:
- Transcript content before writing to disk (configurable — some users want raw transcripts)
- Event data in the stream protocol
- Log messages in the notification engine
- API responses for `/sessions/:id/logs`

Make it configurable with a default-on setting:
```toml
[security]
scrub_transcripts = true    # Scrub before writing to disk
scrub_api_responses = true  # Scrub in API responses
custom_patterns = []        # Additional regex patterns
```

### F. Decision Queue / Human-in-the-Loop Protocol

Mission Control has an explicit decisions queue: agents post questions, humans answer them, and task execution blocks until answered. Forgemux already detects `WaitingInput` state. Extend this:

1. When a session enters `WaitingInput`, check if the agent's JSONL log contains a structured question (e.g., a `permission_request` with options).
2. Parse the question and surface it as a `Decision` in a new hub-level decisions endpoint: `GET /decisions` returns pending decisions with session context.
3. The dashboard shows decisions as a queue, not just a raw terminal view.
4. Answering a decision sends the response to the session via `tmux send-keys` and clears the decision from the queue.

This is a richer interaction model than just "the terminal is waiting." It gives the engineer a structured view of what each agent needs, prioritized by urgency.

### G. Retry with Exponential Backoff

Mission Control's retry system (persistent queue, exponential backoff, max retry count) mapped to Forgemux sessions:

When a session transitions to `Errored`:
1. Check the session's WorkItem (if present) for retry policy
2. If retries remain, create a new session with the same intent, incrementing `attempt` counter
3. Schedule the retry with exponential delay: `base_delay * 2^(attempt - 1)`, capped at max
4. Persist the retry schedule to survive daemon restarts

This could live in forgehub as a policy engine, or in forged as a per-session config:

```toml
[sessions.retry]
max_attempts = 3
base_delay_seconds = 300  # 5 minutes
max_delay_seconds = 3600  # 1 hour
```

### H. Eisenhower-Inspired Priority for Sessions

Mission Control's Eisenhower matrix (importance × urgency) as a session priority model:

Instead of all sessions being equal, add an optional priority field:
```rust
enum SessionPriority {
    Critical,  // Important + Urgent (deploy fix, prod issue)
    High,      // Important + Not Urgent (feature work)
    Medium,    // Not Important + Urgent (dependency updates)
    Low,       // Not Important + Not Urgent (cleanup)
}
```

Priority affects:
- Resource allocation: `maxParallelAgents` could reserve slots for Critical sessions
- Notification urgency: Critical sessions trigger immediate notifications; Low sessions batch
- Dashboard sort order and visual weight
- Foreman attention: supervise Critical sessions more frequently

### I. Context Compression

Mission Control generates `ai-context.md` — a ~650-token snapshot of the entire workspace state (vs ~10,000+ for raw JSON). Forgemux could provide a similar compressed context for:

- Foreman sessions: Instead of reading all raw transcripts, provide a hub-generated summary of all active sessions in ~1000 tokens
- Dashboard data: A lightweight endpoint that returns session counts, states, and alerts without full records
- Agent prompts: A session startup context that summarizes the repo state, recent session outcomes, and known issues

Implementation: `GET /context` or `GET /sessions/summary` returns a pre-computed markdown snapshot. Regenerated on every session state change. Optionally cached with ETag.

### J. Bidirectional Integration Protocol

Rather than making Forgemux replicate Mission Control features, define a protocol for integration:

**Forgemux → Mission Control**:
- Session completes → POST to MC's `/api/tasks/:id` with `kanban: "done"`, `completedAt`
- Session errors → POST to MC's `/api/inbox` with failure report
- Session needs input → POST to MC's `/api/decisions` with question

**Mission Control → Forgemux**:
- Task assigned to agent → POST to Forgemux `/work-items` with task metadata
- Decision answered → POST to Forgemux `/sessions/:id/input` with answer

This can be implemented via Forgemux's existing notification webhook system (for FM→MC) and MC's API routes (for MC→FM), without either project becoming dependent on the other.

---

## Patterns Worth Stealing Wholesale

1. **Atomic rename for file writes**: MC uses `writeFileSync(tmp) → renameSync(tmp, real)`. Forgemux's `SessionStore` should do the same for crash safety.

2. **Stale PID cleanup on timer**: MC checks PIDs every 60s and cleans up dead sessions. Forgemux's forged already checks tmux session liveness, but adding a periodic timer (not just on-demand) would be more proactive.

3. **Prompt fencing**: Wrapping user-provided content in `<task-context>` tags to prevent prompt injection. Forgemux's Foreman prompt should fence session transcripts the same way when injecting them into the meta-agent's context.

4. **Safe environment construction**: MC strips all env vars before spawning agents, keeping only PATH, HOME, TEMP, and system vars. Forgemux could apply the same pattern when launching agent CLIs in tmux sessions to prevent credential leakage.

5. **Retry queue persistence**: MC writes its retry queue to disk (with atomic rename) so retries survive daemon restarts. If Forgemux adds retry logic, persist the schedule.

---

## Patterns to Avoid

1. **Filesystem as IPC**: MC's daemon and web UI communicate through shared JSON files without cross-process locking. This works for single-user local-first but doesn't scale. Forgemux's HTTP/WebSocket API is the right choice for multi-node.

2. **Single-file-per-collection**: MC stores all tasks in one `tasks.json`. At scale this creates write contention and large atomic reads. Forgemux's one-file-per-session is better.

3. **No live observation**: MC's daemon spawns `claude -p` and waits for exit. No visibility into what the agent is doing. Forgemux's PTY capture + state detection is fundamentally superior for operational clarity.

4. **In-process mutex only**: MC's `async-mutex` only works within the Node.js process. The daemon and API server are different processes that can corrupt files. Forgemux should use file locks or database transactions for any shared state.

5. **Slash commands as primary interface**: MC's `/orchestrate`, `/daily-plan`, etc. are Claude Code slash commands that require the user to be in a Claude Code session. Forgemux's CLI (`fmux`) + HTTP API + dashboard provides multiple interface layers, which is more flexible.

---

## What Mission Control Does That Forgemux Shouldn't

- **Full task management UI** (Eisenhower, Kanban, goal hierarchy, brain dump). This is a product-level feature, not infrastructure. Let MC (or Linear, or Jira) own this layer. Forgemux should consume tasks, not manage them.

- **Agent personas with rich instructions and skills**. The concept is useful (see idea B above) but the full registry UI (crew management, skill editing, bidirectional linking) belongs in a higher-level tool. Forgemux should support injecting persona config from files, not building a persona management system.

- **Daemon polling on a timer**. MC polls `tasks.json` every N minutes because there's no event system. Forgemux already has an event-driven architecture (state detection, notifications, WebSocket). If Forgemux adds scheduling, use events, not polling.

---

## Actionable Items (Ranked by Impact / Effort)

| Priority | Item | Effort | Impact |
|----------|------|--------|--------|
| 1 | Credential scrubbing in transcripts/logs | Low | High |
| 2 | Prompt fencing for Foreman transcript injection | Low | Medium |
| 3 | Atomic rename for SessionStore writes | Low | Medium |
| 4 | Post-session hook (on_success/on_failure scripts) | Medium | High |
| 5 | WorkItem model (thin task wrapper for sessions) | Medium | High |
| 6 | Agent persona config (TOML-based, skills from .md files) | Medium | Medium |
| 7 | Decision queue surfaced in dashboard | Medium | High |
| 8 | Retry policy with exponential backoff | Medium | Medium |
| 9 | Scheduled session launcher (cron in forgehub) | Medium | Medium |
| 10 | Context compression endpoint | Low | Medium |
| 11 | Session priority enum | Low | Low |
| 12 | Bidirectional MC integration via webhooks | High | Medium |

---

## Summary

Mission Control v1 review identified the right themes. This deeper review validates those themes and adds specificity from the source code:

- MC's **daemon execution model** (Scheduler → Dispatcher → Runner → Health) is clean and well-factored. The same decomposition maps naturally to Forgemux's hub, but with real session observability instead of fire-and-forget.
- MC's **security practices** (credential scrubbing, prompt fencing, binary whitelist, safe env) are immediately portable and fill gaps in Forgemux's current implementation.
- MC's **data model** (especially Agent + Skills + Decisions) provides a blueprint for optional higher-level features that Forgemux could expose through config files rather than a full management UI.
- The **fundamental architectural difference** remains: MC has no durable session concept. Forgemux's tmux-backed PTY capture, WebSocket attach, state detection, and multi-node support provide capabilities that MC's spawn-and-wait model cannot match. The two systems are complementary.

The highest-value items are the security improvements (1-3), which are low-effort and immediately beneficial, followed by the post-session hook and WorkItem model (4-5), which create the integration surface for higher-level tools like Mission Control.
