# Agent Deck Deep Dive vs Forgemux (Round 2)

Date: 2026-02-27

## Purpose
This is a follow-up to `agent-deck.md`. The first pass identified surface-level feature gaps and idea seeds. This document goes deeper into Agent Deck's implementation details, architecture patterns, and recent additions (Docker sandbox, web mode, conductor improvements) to extract more specific, actionable ideas for Forgemux.

---

## Agent Deck Architecture (Detailed)

### Core Stack
- **Go 1.24** with Bubble Tea TUI (charmbracelet stack: bubbletea, bubbles, lipgloss)
- **SQLite** (pure Go, no CGO via modernc.org/sqlite) for session persistence
- **tmux** as session substrate (same as Forgemux)
- **WebSocket** for browser terminal streaming
- **TOML** for configuration

### Data Model: `Instance`
Agent Deck's session record is richer than Forgemux's `SessionRecord` in several ways:

```
Instance {
    ID, Title, ProjectPath, GroupPath,
    Tool (claude|gemini|opencode|codex),
    Command, TMuxSessionName, Status,
    ClaudeSessionID,        // for fork/resume
    GeminiSessionID,        // for resume
    ParentSessionID,        // sub-session hierarchy
    ParentProjectPath,
    WorktreePath, WorktreeRepoRoot, WorktreeBranch,
    Sandbox *SandboxConfig, // Docker settings
    CreatedAt, UpdatedAt
}
```

Notable differences from Forgemux's `SessionRecord`:
- **Tool-specific session IDs** for fork/resume (Claude conversation ID, Gemini session ID)
- **Parent/child relationships** for sub-session orchestration
- **Worktree fields** as first-class data rather than derived
- **Sandbox config** embedded in the session record
- **GroupPath** for organizational hierarchy (Forgemux has no grouping concept)

### Status Detection
Both projects detect agent state by reading tmux pane output, but Agent Deck uses tool-specific pattern matching:
- Claude: Checks for input bar presence, running spinners, specific prompt patterns
- Gemini: Similar output-based heuristics
- Codex/OpenCode: Content-change-based detection (output changed in last 2s = running)

Forgemux's `StateDetector` is more general (idle timeout + generic pattern matching). Agent Deck's approach is more accurate per-tool but more brittle across tool updates.

---

## Feature Deep Dives

### 1. MCP Socket Pooling
Agent Deck's standout infrastructure feature. Instead of each session spawning its own MCP server processes:

- A shared pool runs MCP servers once, exposing them via Unix sockets
- Sessions connect to the pool via a reconnecting proxy
- Crashes auto-recover in ~3 seconds
- Memory savings: 85-90% reduction when running 10+ sessions
- Configuration: `mcp_pool.enabled = true` in config.toml, with `pool_all` or selective exclusion

**Forgemux relevance:** Forgemux doesn't manage MCP processes today, but if it ever wraps agent configuration, pooling is a proven pattern. More immediately, the Unix socket proxy pattern could be useful for the Forgemux edge daemon's communication with sidecars.

### 2. Docker Sandbox Mode (v0.19.17)
Recent addition. Per-session Docker isolation:

- Read-write bind-mount of the project directory
- SSH mount support for git operations inside container
- Environment variable forwarding
- Auto-cleanup of containers on session termination
- Custom image support per session

**Forgemux relevance:** Forgemux's spec describes cgroup v2 + network namespace isolation, which is lighter weight. Agent Deck chose Docker for simplicity (no cgroup configuration needed, works on macOS). The tradeoff: Docker is heavier but more portable. Forgemux could offer both: cgroups for Linux edge nodes, Docker as a cross-platform fallback.

### 3. Web Mode + PWA (v0.19.0)
Agent Deck runs a WebSocket server alongside the TUI:

- Browser terminal via xterm.js equivalent (vanilla JS + ansi-to-html)
- REST API for session metadata (list, status)
- Web push notifications via VAPID
- Bearer token auth
- Same-origin CORS
- PWA manifest for mobile home screen

**Forgemux relevance:** Forgemux's browser attach is planned as a hub-mediated relay. Agent Deck's simpler approach (direct WebSocket from browser to local process) works well for single-node. Forgemux's relay approach is better for multi-node but more complex. The PWA + push notification pattern is worth borrowing for the Forgemux dashboard.

### 4. Conductor System (Orchestration)
Agent Deck's conductor is more mature than described in the first review:

- **Persistent Claude Code session** that monitors other sessions
- **Heartbeat-driven monitoring** with configurable rules and intervals
- **Event-driven notifications** on session state transitions
- **Telegram/Slack bridge** for remote control (Python subprocess)
- **Multi-profile support** with per-conductor configuration
- **Context management** to avoid context window overflow (v0.19.15)
- **Permission loop fixes** to prevent conductor from getting stuck (v0.19.19)

The conductor is conceptually identical to Forgemux's Foreman concept, but Agent Deck has shipped it and iterated on real-world problems (context overflow, permission loops, crash recovery).

**Forgemux relevance:** The Foreman spec should incorporate lessons from Agent Deck's conductor:
- Context management is critical: the foreman's context window fills up watching many sessions
- Permission loops: the foreman can trigger tool-use permission prompts that block it
- Heartbeat intervals need tuning: too frequent wastes tokens, too infrequent misses transitions
- Remote bridges (Telegram/Slack) are a compelling UX for "fire and forget" agent supervision

### 5. Session Forking
Agent Deck implements fork by:
1. Reading the Claude Code conversation ID from the session
2. Creating a new session with `--resume <conversation_id>`
3. Optionally creating a new git worktree for the fork

This preserves full conversation context in the forked session. It's Claude-specific.

**Forgemux relevance:** Forgemux's fork concept from the first review was "clone transcript + seed context." Agent Deck's approach is simpler and more faithful: just use the agent's own resume mechanism. Forgemux should support `fmux fork` that:
1. Reads the agent's session/conversation ID (from JSONL logs or structured output)
2. Starts a new session with the agent's resume flag
3. Optionally creates a worktree

### 6. Profile System
Agent Deck supports multiple isolated profiles:

```toml
[profiles.work.claude]
config_dir = "~/.claude-work"
```

Each profile gets separate:
- Claude config directory (different API keys, settings)
- MCP configuration
- Tool defaults
- Conductor instances
- SQLite database

Multiple agent-deck instances can run simultaneously with `allow_multiple = true`.

**Forgemux relevance:** Forgemux doesn't have profiles. For multi-tenant edge nodes or developers working across multiple orgs, a profile concept would be useful. Could be implemented as namespaced config directories and session stores.

### 7. Skills Manager
Agent Deck manages Claude skills as a pool:

- Skills live in `~/.agent-deck/skills/pool/`
- Per-project state in `.agent-deck/skills.toml`
- Materialized as symlinks in `.claude/skills/`
- Attach/detach via TUI dialog or CLI

**Forgemux relevance:** This is agent-specific tooling that Forgemux probably shouldn't replicate. But the pattern of "managed attachments that get materialized into agent config" is useful. Forgemux could generalize this as "session plugins" that inject config/env/mounts at session start.

### 8. Git Worktree Integration
Full lifecycle management:

- Create worktree with configurable location (sibling, subdirectory, custom path template)
- Branch creation and validation
- Orphaned worktree detection and cleanup
- Tight coupling with session lifecycle: worktree created at session start, cleaned up on removal

**Forgemux relevance:** Forgemux has `--worktree` flags but no lifecycle management. Agent Deck's approach of tying worktree lifecycle to session lifecycle is clean. `fmux start --worktree feature-x` should create the worktree, and `fmux stop --cleanup` should offer to remove it.

---

## Architectural Patterns Worth Borrowing

### 1. SQLite for Session Storage
Agent Deck uses SQLite with transactions and concurrent access. Forgemux uses JSON files with optimistic concurrency (version field). SQLite would give Forgemux:
- Atomic multi-record updates
- Query capability (find sessions by status, agent, repo)
- Better concurrent access from multiple processes
- Migration support for schema evolution

The counter-argument: JSON files are simpler and more transparent for debugging. But as Forgemux grows (hub aggregation, foreman queries), SQLite becomes more natural.

### 2. Structured Logging with Ring Buffer
Agent Deck's JSONL logs with ring buffer and SIGUSR1 crash dumps are a mature debugging pattern. Forgemux uses the `tracing` crate which is excellent for structured logging but doesn't have the ring buffer / crash dump pattern. Adding a `tracing` subscriber that maintains a ring buffer for post-mortem analysis would be valuable for edge daemon debugging.

### 3. Primary Election for Single Instance
Agent Deck uses SQLite-based primary election to prevent multiple instances from managing the same sessions. Forgemux doesn't have this concern today (edge daemon is a singleton), but if multiple forged instances could run on the same node (e.g., per-user), a locking mechanism would be needed.

---

## Gaps in Agent Deck That Forgemux Fills

To be fair, Agent Deck has significant gaps that Forgemux addresses:

| Gap in Agent Deck | Forgemux Solution |
|---|---|
| No multi-node support | Hub + edge federation |
| No reliable reconnection protocol | RESUME/EVENT/INPUT/ACK stream protocol |
| Sessions lost on reboot (tmux dies) | Durable event ring + snapshots for replay |
| No stream encryption | AES-GCM encrypted streams |
| No version compatibility checks | CLI/hub version negotiation (426 on mismatch) |
| No optimistic concurrency | SessionRecord.version field |
| Web mode is local-only | Hub-mediated WebSocket relay (NAT traversal) |
| Status detection is polling-based | Event-driven state machine with idle timeouts |
| No token usage tracking | JSONL log parsing for Claude/Codex usage |
| No cost estimation | Token-to-cost mapping |

---

## New Idea Seeds for Forgemux

Building on the first review's ideas, here are deeper and more specific proposals:

### Idea 1: Session Groups and Hierarchies
**From:** Agent Deck's GroupPath and parent/child sessions

Agent Deck organizes sessions into tree-structured groups and supports parent-child relationships. Forgemux could add:

- `SessionRecord.group: Option<String>` for organizational grouping
- `SessionRecord.parent_id: Option<SessionId>` for sub-session relationships
- `fmux start --group "project-alpha"` and `fmux ls --group "project-alpha"`
- Dashboard grouping in the hub UI
- Foreman could be scoped to a group rather than all sessions

This is low-cost to implement (two optional fields on SessionRecord) and high-value for multi-project workflows.

### Idea 2: Agent Resume/Fork via Structured Logs
**From:** Agent Deck's ClaudeSessionID-based forking

Forgemux already parses agent JSONL logs for token usage. Extend this to extract conversation/session IDs:

- Claude Code: conversation ID from JSONL
- Codex: session ID from JSONL

Then implement:
- `fmux fork <session-id>` - creates new session with `--resume <conversation-id>`
- `fmux resume <session-id>` - restarts a terminated session with its conversation ID
- Store extracted IDs in `SessionRecord.agent_session_id: Option<String>`

### Idea 3: MCP-Aware Session Configuration
**From:** Agent Deck's MCP Manager and pool

Rather than managing MCP processes directly (Agent Deck's approach), Forgemux could:

- Allow session-level MCP config overrides: `fmux start --mcp-config ./custom-mcps.json`
- Store MCP config reference in SessionRecord
- On session start, merge base MCP config with session overrides
- Expose MCP config in dashboard for visibility

This stays agent-agnostic while giving operators control over per-session tool access.

### Idea 4: Lightweight TUI via ratatui
**From:** Agent Deck's Bubble Tea TUI

Forgemux is CLI-first, but a thin TUI would significantly improve daily use:

- Use `ratatui` (Rust equivalent of Bubble Tea)
- Session list with real-time status updates
- Quick attach via selection
- Inline log tail
- Filter by status, agent, group

Scope: This is a separate `fmux tui` command, not a replacement for the CLI. It reads from the same edge API that the dashboard uses.

### Idea 5: Push Notifications via Web Push
**From:** Agent Deck's PWA + VAPID push notifications

Forgemux's dashboard could support web push:

- VAPID key generation during hub setup
- Subscription management endpoint
- Push on session state transitions (WaitingInput, Errored, Terminated)
- Configurable notification rules per user

This complements the existing webhook notification system with browser-native delivery.

### Idea 6: Session Templates
**From:** Agent Deck's profile system and per-tool defaults

Rather than profiles, Forgemux could support session templates:

```toml
[templates.backend-worker]
agent = "claude"
model = "sonnet"
sandbox = "cgroup"
cpu_limit = "2.0"
memory_limit = "4G"
notifications = ["webhook", "desktop"]

[templates.frontend-worker]
agent = "claude"
model = "sonnet"
sandbox = "docker"
image = "node:20"
```

Usage: `fmux start --template backend-worker --repo .`

Templates reduce boilerplate and enforce consistency across team sessions.

### Idea 7: Foreman Context Management
**From:** Agent Deck's conductor context overflow fixes (v0.19.15)

The Foreman spec should include explicit context management:

- Sliding window over watched session transcripts (only recent N lines per session)
- Summary checkpoints: periodically compress old transcript into a summary
- Priority ordering: WaitingInput and Errored sessions get more context budget
- Token budget per foreman cycle (e.g., max 50k tokens of transcript per evaluation)
- Configurable in ForgedConfig

### Idea 8: Remote Control Bridge
**From:** Agent Deck's Telegram/Slack conductor bridge

Forgemux's hub could expose a bot integration:

- Telegram bot: `/sessions` list, `/attach <id>` link, `/stop <id>`, alerts on state change
- Slack app: Similar commands via slash commands or Socket Mode
- Implementation: lightweight sidecar or built into forgehub

This is especially useful for Forgemux's multi-node scenario where engineers may not be at their terminal.

### Idea 9: Worktree Lifecycle Management
**From:** Agent Deck's worktree create/cleanup automation

Extend Forgemux's existing `--worktree` flag:

- `fmux start --worktree feature-x` creates worktree + starts session in it
- `fmux stop <id> --cleanup-worktree` removes worktree after session ends
- `fmux worktree list` shows worktrees with their associated sessions
- `fmux worktree cleanup` finds orphaned worktrees (session terminated but worktree remains)
- Store worktree info in SessionRecord for tracking

### Idea 10: Session Search Across Nodes
**From:** Agent Deck's fuzzy local + regex global search

Forgemux's hub aggregates sessions from all edges. Add search:

- `fmux search "auth"` - searches session titles, repos, transcripts across all nodes
- `fmux search --status waiting` - find all sessions waiting for input
- `fmux search --agent codex --node mel-01` - filtered search
- Hub indexes session metadata; transcript search fans out to edges
- Dashboard search bar with the same capabilities

---

## Priority Matrix

| Idea | Effort | Impact | Phase |
|---|---|---|---|
| Session groups/hierarchy | Low | Medium | 0-1 |
| Agent resume/fork | Low | High | 1 |
| Worktree lifecycle | Low | Medium | 1 |
| Session templates | Medium | Medium | 1-2 |
| Lightweight TUI (ratatui) | Medium | High | 1-2 |
| Session search | Medium | High | 2 |
| Foreman context mgmt | Medium | High | 2 |
| MCP-aware session config | Medium | Low | 2 |
| Push notifications (web) | Low | Medium | 3 |
| Remote control bridge | High | Medium | 3 |

---

## Strategic Observations

1. **Agent Deck is converging toward Forgemux's problem space.** Its web mode, conductor system, and Docker sandbox are all moves toward durability and remote access. But it's building on a single-node Go architecture that doesn't naturally extend to multi-node federation.

2. **Forgemux's edge/hub split is a structural advantage.** Agent Deck would need a significant rewrite to support multi-node. Forgemux was designed for it from day one.

3. **Agent Deck's iteration speed is a threat.** It ships features weekly (v0.19.x releases every 1-2 days). Forgemux should prioritize the features that compound on its architectural advantages (hub search, cross-node foreman, reliable stream protocol) rather than trying to match Agent Deck feature-for-feature on single-node UX.

4. **The MCP pooling pattern is genuinely novel.** If Forgemux sessions start managing agent tool configurations, the Unix socket pooling approach should be adopted. 85-90% memory reduction is significant at scale.

5. **Conductor/Foreman is the battleground.** Both projects see meta-agent supervision as a key feature. Agent Deck has shipped and iterated. Forgemux's Foreman spec is more ambitious (three intervention levels, hub integration) but needs to incorporate the practical lessons Agent Deck has learned: context overflow, permission loops, crash recovery.

6. **Agent Deck has better single-user ergonomics; Forgemux has better infrastructure.** The ideal path for Forgemux is to add a thin ergonomics layer (TUI, search, templates) on top of its solid protocol and state machine foundation, rather than rebuilding the infrastructure to be more "user friendly."
