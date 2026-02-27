# claudedash Deep Dive: Round 2

> Follows up on `claudedash.md` (round 1) with a deeper code-level review.
> Focuses on implementation patterns, architectural ideas, and concrete
> features that forgemux could adopt or adapt.

## Executive Summary

Round 1 identified high-level concepts (plan mode, quality gates, context
health, worktree observability). This round goes deeper into claudedash's
implementation to surface **patterns, subsystems, and UX details** that
forgemux hasn't considered yet.

The biggest opportunities fall into three categories:

1. **Recovery and resilience** -- claudedash has a snapshot/rollback system
   that forgemux's transcript layer could evolve into.
2. **Agent self-introspection** -- claudedash exposes an MCP server so the
   agent can query its own state; forgemux's Foreman could benefit from the
   same pattern.
3. **Operational polish** -- hook lifecycle management, mtime caching,
   billing-window awareness, and a `doctor` diagnostic command that are all
   cheap to build and high-value for operators.

---

## New Observations (not covered in round 1)

### 1. Context Snapshot and Rollback System

claudedash captures structured snapshots keyed by git commit hash:

```
.claudedash/snapshots/<commit-hash>.json
  ├── git state (branch, dirty, diff summary)
  ├── task state (queue snapshot, execution log slice)
  ├── task timeline (status transitions)
  └── recent execution log entries
```

Recovery is a CLI command (`claudedash recover <hash>`) that prints a summary
and recommends `git reset --hard`. Snapshots are triggered automatically on
commit via hooks.

**Forgemux relevance:** Forgemux already captures transcripts and persists
session records to disk. It could extend this into a first-class recovery
system:

- **Snapshot = session record + transcript tail + git state + worktree diff**.
- Store snapshots in `data_dir/snapshots/S-xxxxx/<commit>.json`.
- `fmux recover S-xxxxx` prints state and offers to reset the worktree.
- Foreman could trigger snapshots automatically before risky operations.

This bridges forgemux's infrastructure-level persistence with claudedash's
task-level recovery -- giving operators a "time travel" capability.

### 2. MCP Server for Agent Self-Introspection

claudedash ships an MCP server (JSON-RPC over stdio) that exposes tools:

| Tool              | Purpose                           |
|-------------------|-----------------------------------|
| `get_queue`       | Read current task queue state     |
| `get_sessions`    | List active sessions              |
| `get_cost`        | Token costs by model              |
| `log_task`        | Write a task result               |
| `create_task`     | Add a task to the queue           |
| `register_agent`  | Register agent with heartbeat     |
| `send_heartbeat`  | Keep agent alive in registry      |

This lets the agent itself query "how am I doing?" and take corrective action.

**Forgemux relevance:** The Foreman already acts as a supervisor. Adding an
MCP server to `forged` (or a lightweight HTTP introspection endpoint) would
let any managed session query:

- Its own token usage and remaining budget.
- Other session states (for coordination).
- Worktree status and pending merges.
- Whether the Foreman has flagged it for intervention.

This would enable **cooperative multi-agent patterns** where sessions
voluntarily yield, checkpoint, or request help from the Foreman -- rather
than relying solely on external observation.

### 3. Hook Lifecycle Management

claudedash has a full hook management subsystem:

```
claudedash hooks install   # writes to ~/.claude/settings.json
claudedash hooks status    # shows installed hooks
claudedash hooks uninstall # removes hooks
```

Hooks fire on Claude Code events:
- **PostToolUse** (after Bash/Edit/Write) -- event logging
- **Stop** (agent finishing) -- warns if tasks remain
- **PreCompact** (before context compaction) -- auto-commit + snapshot
- **PostCompact** (after compaction) -- injects recovery reminder into CLAUDE.md

The hook service maintains a ring buffer (100 events) for dashboard display.

**Forgemux relevance:** Forgemux could define its own hook protocol:

- `forged hooks install` writes Claude Code hooks that POST to the local
  forged instance instead of claudedash.
- PreCompact hook triggers snapshot capture (see idea #1 above).
- PostToolUse hooks feed the agent log watcher with richer signals than
  JSONL tail-reading alone.
- Stop hooks could notify the Foreman that a session is about to terminate.

This is lower-friction than JSONL parsing for state detection and gives
forgemux a direct integration point with Claude Code's lifecycle.

### 4. Billing Window Awareness

claudedash tracks 5-hour billing windows for Claude API usage:

```
GET /billing-block → { start, end, tokens_used, cost_usd, remaining_pct }
```

This is consumed from Claude's `stats-cache.json` file.

**Forgemux relevance:** The usage collector already aggregates token counts.
Adding billing-window awareness would let the dashboard and Foreman answer:

- "How much budget is left in this billing window?"
- "Should we throttle session creation to avoid overage?"
- "Which sessions are consuming disproportionate budget?"

Implementation: Read Claude's `stats-cache.json` alongside the existing JSONL
parsing. Expose as a field on the hub dashboard and as a Foreman input.

### 5. Agent Registry with Heartbeat

claudedash maintains a registry of agents:

```
POST /agent/register  { name, taskId, sessionId }
→ agent appears as "alive" in dashboard
→ heartbeat every 60s keeps it active
→ timeout → agent marked dead
```

**Forgemux relevance:** Forgemux tracks sessions but not the agent process
inside each session independently. The agent could crash while tmux keeps
running, creating a "zombie session" that looks alive but isn't doing work.

Adding agent-level heartbeats (via hook or log watcher) would improve
StateDetector accuracy. A session with a live tmux pane but no agent
heartbeat could be classified as `Errored` or `Stalled` rather than
`Running`.

### 6. Insights Engine (Velocity, Bottlenecks, Success Rate)

claudedash computes analytics from execution logs:

- **Timeline**: Task status transitions over time.
- **Velocity**: Tasks completed per time window.
- **Bottlenecks**: Tasks that block the most others.
- **Success rate**: DONE vs FAILED ratio.

**Forgemux relevance:** Forgemux collects session lifecycle events and token
usage but doesn't compute derived insights. A lightweight insights layer
could produce:

- **Session velocity**: Sessions completed per day/week.
- **Stall frequency**: How often sessions hit WaitingInput or Idle.
- **Cost efficiency**: Tokens per completed session (proxy for ROI).
- **Bottleneck sessions**: Sessions that Foreman intervenes on most.
- **Agent comparison**: Claude vs Codex success rates and cost.

These could be dashboard widgets or CLI summary output (`fmux stats`).

### 7. Stale Task Detection and Dismissal

claudedash marks tasks as "stale" if they've been `in_progress` for >24h
without activity, and lets users dismiss them from the Kanban view:

```
DELETE /sessions/:id/tasks/:taskId
```

**Forgemux relevance:** Forgemux's idle detection already handles session-level
staleness. But surfacing stale *tasks within sessions* (from Claude Code's
todo list) would give the dashboard more granular information. A user could
dismiss a stale session from the dashboard without SSH-ing into the edge node.

### 8. Prompt History Endpoint

claudedash serves user prompt history from `history.jsonl`:

```
GET /history → [{ prompt, timestamp, sessionId, model }]
```

Cached by mtime to avoid re-parsing.

**Forgemux relevance:** Forgemux captures full transcripts but doesn't index
them by prompt. A prompt history view would let operators:

- Search across sessions by prompt content.
- See what instructions led to stalls or failures.
- Build a "session replay" starting from the initial prompt.

This could be extracted from transcripts or from Claude's history.jsonl if
accessible on the edge node.

---

## Implementation Patterns Worth Adopting

### A. mtime-Based Endpoint Caching

claudedash checks file mtimes before re-parsing:

```typescript
const mtime = fs.statSync(path).mtimeMs;
if (mtime === lastMtime) return cachedResult;
// else re-parse and update cache
```

Forgemux's `SessionStore` currently reads from disk on every load. Adding
mtime checks to `load()` and `load_all()` would reduce I/O on the hot path
(especially `GET /sessions` which loads all session records).

### B. Tail-Read for Large Files

claudedash reads session JSONL files backwards from EOF, loading only the
last N lines. This avoids loading multi-MB log files into memory.

Forgemux's agent log watcher (`AgentLogWatcher`) currently reads from a
tracked position forward, which is fine for watching. But for initial load
(e.g., when forged restarts), a tail-read would be more efficient than
reading from the beginning.

### C. SSE with Per-Client Ping

claudedash's `SseHub` sends a ping every 30s per client to prevent connection
drops through proxies and load balancers.

Forgemux uses WebSocket for browser attach, which has its own ping/pong. But
if forgemux adds an SSE endpoint for dashboard updates (lighter than WS for
read-only streams), this pattern is worth adopting.

### D. Ring Buffer for Recent Events

claudedash's `HookService` keeps a ring buffer of the last 100 hook events.
Forgemux's `EventRing` already implements this pattern for stream events.
This validates the approach and suggests extending it to other event types
(notifications, state transitions, Foreman decisions).

---

## Feature Ideas for Forgemux (Prioritized)

### Tier 1: Low effort, high value

1. **`fmux doctor` expansion** -- claudedash's `doctor` command checks
   environment health (Claude CLI version, file permissions, config
   validity). Forgemux has `forged check` but could add: agent CLI version
   checks, tmux version compatibility, cgroup delegation status, TLS cert
   expiry, hub connectivity, and disk space for transcripts.

2. **Session record mtime caching** -- Skip re-parsing unchanged session
   JSON files in `SessionStore::load_all()`. Check mtime before
   deserialization.

3. **Billing window awareness** -- Parse `stats-cache.json` alongside
   usage JSONL. Expose remaining budget on dashboard and to Foreman.

4. **Agent heartbeat signal** -- Use Claude Code hooks or log watcher to
   detect agent-alive vs tmux-alive divergence. Improves StateDetector
   accuracy for zombie sessions.

### Tier 2: Medium effort, high value

5. **Context snapshot system** -- Extend session records with periodic
   snapshots (git state + transcript tail + session state). Keyed by
   commit hash. `fmux snapshot S-xxxxx` to capture, `fmux recover` to
   restore. Foreman could auto-snapshot before interventions.

6. **Hook integration with Claude Code** -- `forged hooks install` writes
   Claude Code hooks that POST to `forged`. PreCompact triggers snapshot.
   PostToolUse feeds state detector. Stop notifies Foreman.

7. **Prompt history index** -- Extract prompts from transcripts or
   Claude's history.jsonl. Serve via hub API. Enable cross-session search.

8. **Session insights dashboard** -- Compute velocity, stall frequency,
   cost efficiency, agent comparison from existing lifecycle events and
   usage data. Display as dashboard widgets.

### Tier 3: Higher effort, strategic value

9. **MCP introspection endpoint on forged** -- Let managed sessions query
   their own state, other session states, and Foreman status. Enables
   cooperative multi-agent coordination without Foreman as sole observer.

10. **Task-level observability** -- Read Claude Code's todo/task files
    from within sessions (requires filesystem access to session worktrees).
    Surface task-level progress alongside session-level state in dashboard.

---

## Architectural Comparison Matrix

| Dimension               | claudedash                    | forgemux                        | Gap / Opportunity                    |
|--------------------------|-------------------------------|---------------------------------|--------------------------------------|
| Runtime model            | Observer (read-only)          | Orchestrator (lifecycle owner)  | --                                   |
| Deployment               | Single-node, zero-infra       | Hub + edge, multi-node          | --                                   |
| Data source              | Filesystem (Claude artifacts) | Managed processes + filesystem  | forgemux has richer raw data         |
| Recovery                 | Snapshot + git rollback       | Transcripts only                | Add structured snapshots             |
| Self-introspection       | MCP server for agent          | Foreman (external observer)     | Add introspection endpoint           |
| Hook integration         | Full lifecycle hooks          | Log watching only               | Add hook-based state detection       |
| Billing awareness        | 5-hour windows                | Raw token counts                | Add billing window tracking          |
| Agent liveness           | Heartbeat registry            | tmux process check              | Add agent-level heartbeat            |
| Derived insights         | Velocity, bottlenecks, rates  | Raw metrics only                | Add insights engine                  |
| Stale detection          | Task-level (24h threshold)    | Session-level (idle timeout)    | Add task-level staleness             |
| Dashboard caching        | mtime + SSE                   | WebSocket                       | Add mtime caching for REST endpoints |
| Prompt searchability     | History endpoint              | Transcripts (unindexed)         | Index prompts from transcripts       |

---

## Key Takeaway

claudedash and forgemux occupy complementary positions: claudedash observes
from outside, forgemux controls from inside. The largest opportunity is to
bring claudedash's **observability depth** (snapshots, hooks, introspection,
insights) into forgemux's **orchestration framework** -- giving forgemux not
just control over sessions, but rich understanding of what's happening
inside them.

The top three ideas with the best effort-to-impact ratio:

1. **Hook integration** (#6) -- direct Claude Code lifecycle events into
   forged, replacing indirect log parsing with explicit signals.
2. **Context snapshots** (#5) -- structured recovery points that leverage
   forgemux's existing transcript and git worktree infrastructure.
3. **Agent heartbeat** (#4) -- distinguish "tmux alive" from "agent alive"
   to eliminate zombie session blind spots.

---

Sources reviewed:
- claudedash: full source tree (`src/`, `dashboard/`, `docs/`, `package.json`)
- forgemux: full source tree (`crates/`, `dashboard/`, `docs/`, `Cargo.toml`)
- Prior analysis: `docs/research/claudedash.md`
