# User Scenario Brief: Agent Session Orchestrator

**Document type:** User Scenario Brief
**Date:** 2025-02-25
**Author:** Jonathan Chang — Silverpond
**Related:** Product Briefing — Agent Session Orchestrator (February 2026)

---

## Overview

The Agent Session Orchestrator wraps AI coding agents (Claude Code, Codex CLI) in a durable, observable session layer running at the edge. An engineer starts an agent session with a single command or browser click, and the orchestrator handles the rest: git worktree isolation, tmux-backed session durability, credential injection, transcript capture, and telemetry — all without degrading the native terminal experience.

The engineer's workflow reduces to three operations at most: **start**, **list**, **attach**. The organisation gains visibility, control, and auditability over every agent session.

## Actors

| Actor | Role |
|---|---|
| Engineer (Jono) | Primary user; starts, attaches, and works with agent sessions |
| Tech lead / Reviewer | Observes sessions read-only for review or pair-debugging |
| Engineering manager | Monitors team agent activity, cost, and usage via the dashboard |
| Edge node (`edge-01`) | Remote machine where code, data, and agent sessions reside |
| `edgectl` CLI | Control-plane client; issues commands from the engineer's laptop |
| Edge daemon (`edged`) | Manages session lifecycle on a single edge node; enforces local policy |
| Session sidecar | Lightweight process that monitors session state, captures transcripts, and reports metrics |
| `tmux` | Durable session substrate; preserves raw PTY fidelity; multiplexes viewers |
| WebSocket bridge | Translates tmux PTY I/O to xterm.js in the browser; handles backpressure |
| Control plane API | Aggregates session state across edge nodes; serves dashboard and management endpoints |
| Dashboard (Web UI) | Browser-based console for observation, management, and session attach |

## Preconditions

- `edged` is running on `edge-01`.
- `tmux` and `git` are installed on the edge node.
- Agent CLI (Claude Code or Codex CLI) is installed unmodified on the edge node.
- Agent credentials are present on the edge node and never leave it.
- Source repos are cloned under `/data/repos/` on the edge node.
- SSH access to `edge-01` is configured for the engineer.
- A reverse proxy exposes `https://edge.company.com` with authenticated browser access.
- All edge-to-hub communication is encrypted and authenticated.

---

## Scenario A — Production Happy Path (CLI)

### A1. Start a session

The engineer launches a Claude Code session targeting a specific repo.

```
edgectl start claude --repo ~/repos/highlighter
```

```
Session: S-a83f
Edge: edge-01
Worktree: /data/worktrees/S-a83f
Attach (ssh): ssh edge-01 -t 'tmux attach -t S-a83f'
Attach (web): https://edge.company.com/s/S-a83f
```

**Internally:** `edgectl` calls `POST /sessions`. On `edge-01`, `edged` creates a git worktree from the base repo into a session-scoped directory (`/data/worktrees/S-a83f`), injects credentials, and creates a detached tmux session running the agent CLI against the worktree. A session sidecar starts alongside, capturing terminal output for the transcript and reporting state and metrics to the control plane. The session is registered in the dashboard and the web endpoint becomes available. Other sessions targeting the same repo get their own worktrees.

**Commands so far: 1**

### A2. SSH attach

The engineer attaches from the terminal.

```
ssh edge-01 -t 'tmux attach -t S-a83f'
```

They land directly inside the Claude Code CLI, issue prompts, observe responses, and detach with `Ctrl-b d`. The agent continues running and the sidecar continues capturing the transcript.

**Commands so far: 2**

### A3. Web attach

The engineer opens a browser to `https://edge.company.com/s/S-a83f`. The browser authenticates before attach is permitted.

The WebSocket bridge connects to the tmux session via `wss://edge.company.com/ws/sessions/S-a83f/pty`. The raw terminal stream flows over the socket — the engineer sees the same live session as SSH with full PTY fidelity.

If both SSH and web clients are connected simultaneously, tmux handles multiplexing natively. No additional commands are required.

**Commands so far: 2** (web attach is zero-command)

---

## Scenario B — Codex Agent

Identical to Scenario A with a different agent target.

```
edgectl start codex --repo ~/repos/highlighter
```

Agent CLIs are run unmodified. All attach, dashboard, transcript, and policy behaviour is the same regardless of agent type.

---

## Scenario C — Reattach After Disconnect

An engineer starts a session, closes their laptop, and returns days later. The session survives because tmux preserves it on the edge node — no context is lost.

```
edgectl list
```

```
S-a83f  claude  running  edge=edge-01  repo=highlighter  worktree=/data/worktrees/S-a83f  last_active=5m ago
S-b22x  codex   idle     edge=edge-01  repo=hl-runtime   worktree=/data/worktrees/S-b22x  last_active=2d ago
```

They reattach to an idle session.

```
edgectl attach S-b22x
```

This expands internally to `ssh edge-01 -t 'tmux attach -t S-b22x'`. The engineer picks up exactly where they left off.

**Total commands across the entire lifecycle: start → list → attach (3 max)**

---

## Scenario D — Dashboard View

The engineering manager opens `https://edge.company.com/dashboard` and sees a live overview of all sessions across edge nodes.

| Session | Edge | Agent | Model | State | Attached | Worktree | Tokens | Est. Cost | Duration | CPU | Memory | Last Active |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| S-a83f | edge-01 | Claude | claude-3.7-sonnet | running | 1 ssh + 1 web | `/data/worktrees/S-a83f` | 14k | $0.42 | 3h 12m | 32% | 1.2 GB | 5m ago |
| S-b22x | edge-01 | Codex | codex-mini | idle | 0 | `/data/worktrees/S-b22x` | 8k | $0.18 | 1d 4h | 4% | 900 MB | 2d ago |

**State** is derived from tmux session existence, sidecar heartbeat, and PTY activity timestamps. Session states follow the lifecycle defined in the product briefing: `running`, `idle`, `waiting_for_input`, `error`. **Token stats** prefer provider-native reporting; fall back to estimation only when necessary. **Cost** is attributed per session and aggregated across nodes.

---

## Scenario E — Start a Session from the Browser (No CLI)

This scenario covers a fully browser-native workflow. The engineer never opens a terminal to start or attach to a session.

### Preconditions (additional to global)

- The web console is reachable at `https://console.company.com` (or `https://edge.company.com` if no hub).
- The user is authenticated (SSO).
- `edge-01` is online and registered with the control plane (or directly reachable).

### E1. Start a session via the web console

The engineer performs the following clicks — no terminal required.

1. Open `https://console.company.com`.
2. Click **Sessions**.
3. Click **New Session**.
4. Choose edge node (`edge-01`), agent (`Claude Code` or `Codex CLI`), model, and workspace (`highlighter`).
5. Click **Start**.

A session card appears immediately showing `S-a83f` with status transitioning from `starting` to `running`, along with **Attach (Web)**, **Attach (SSH)**, and **Copy** buttons.

**Commands so far: 0**

### E2. Backend start path

The browser submits a session creation request to the control plane API.

```
POST /api/sessions
{
  "edge_id": "edge-01",
  "agent": { "type": "claude", "model": "claude-3.7-sonnet" },
  "workspace": {
    "name": "highlighter",
    "base_repo": "/data/repos/highlighter",
    "branch": "main"
  },
  "title": "highlighter investigation"
}
```

The control plane forwards a start instruction to `edged` on `edge-01` over an existing encrypted channel (direct HTTPS if reachable, or a persistent outbound tunnel from edge to hub). `edged` creates an isolated git worktree, injects credentials, and starts the agent inside it:

```
git -C /data/repos/highlighter worktree add \
  /data/worktrees/S-a83f --detach HEAD

tmux new-session -d -s S-a83f \
  sidecar run claude --repo /data/worktrees/S-a83f --model claude-3.7-sonnet
```

The sidecar spawns the agent CLI in a PTY within the tmux session and begins capturing the transcript. The edge reports `SessionCreated` and `StateChanged(running)` events, followed by periodic heartbeats. If the agent produces output, `OutputChunk` events stream up to the control plane for the live UI.

### E3. Attach from the browser

The engineer clicks **Attach (Web)** on the session card. The browser authenticates, then a terminal pane opens inline in the web UI.

The WebSocket bridge connects via `wss://console.company.com/ws/sessions/S-a83f/pty`. The control plane routes the stream to `edge-01`, where `edged` allocates a PTY and attaches to the tmux session.

The interactive loop is:

- Keystrokes → WebSocket → PTY stdin → tmux → agent
- Output bytes → tmux → PTY stdout → WebSocket → browser terminal (xterm.js)

**Commands so far: 0** (all interaction is click and type within the browser)

### E4. Optional SSH attach

At any point the engineer can click **Copy SSH attach** on the session card to copy:

```
ssh edge-01 -t 'tmux attach -t S-a83f'
```

Running this in a local terminal attaches alongside the web session. tmux multiplexes both clients concurrently.

### E5. Dashboard live state

The session card and dashboard list show live telemetry:

| Field | Source |
|---|---|
| Edge | Registration / control channel |
| Agent + Model | Session config |
| State (`running` / `idle` / `waiting_for_input` / `error`) | tmux existence + sidecar heartbeat + PTY activity |
| Worktree | Path to session-scoped git worktree |
| Attached (`web:1, ssh:0`) | tmux client count + active WebSocket connections |
| Tokens (input / output) | Provider-native reporting; estimation fallback |
| Estimated cost | Calculated from token usage per provider pricing |
| Duration / active time | Session start timestamp + cumulative active intervals |
| CPU / Memory | Edge node resource monitoring |
| Last activity | Most recent input/output event timestamp |

### Fewer-clicks variant: Start & Attach

The **New Session** modal can offer a single **Start & Attach** button. This submits `POST /api/sessions`, waits for `running` state, then immediately opens the WebSocket terminal — collapsing E1–E3 into a single click after configuration.

---

## Scenario F — Read-Only Review Attach

A tech lead wants to observe an agent session without interrupting the engineer running it.

### F1. From the dashboard

The tech lead opens the dashboard and sees session `S-a83f` is running. They click **Observe** on the session card.

A read-only terminal pane opens in the browser. The tech lead can see everything the agent is doing in real time but cannot send keystrokes. The engineer is not interrupted and may not even know someone is watching.

### F2. From SSH

Alternatively, the tech lead runs:

```
ssh edge-01 -t 'tmux attach -t S-a83f -r'
```

The `-r` flag makes the tmux attach read-only. The tech lead sees live output but cannot interact.

### F3. Multiplexing

Multiple observers can attach simultaneously (read-only web, read-only SSH, or mixed). The engineer retains full interactive control. tmux handles all multiplexing.

**Use cases:** code review of agent work in progress, pair-debugging without screen sharing, training and onboarding.

---

## Scenario G — Session Termination

Sessions can be terminated in three ways: by the engineer, by policy, or by an administrator.

### G1. Engineer terminates

From the CLI:

```
edgectl stop S-a83f
```

Or from the dashboard: click **Terminate** on the session card.

`edged` kills the tmux session, the sidecar flushes the final transcript and lifecycle event (`SessionTerminated`), and the git worktree is cleaned up (`git worktree remove`).

### G2. Idle timeout policy

A session policy specifies a maximum idle duration (e.g. 4 hours with no PTY activity). When the timeout fires, `edged` terminates the session automatically and records the reason in the lifecycle log.

### G3. Administrator remote kill

An engineering manager or platform admin clicks **Terminate** on any session from the dashboard. This sends a terminate instruction via the control plane to the edge node. The session is stopped, transcripts are flushed, and the worktree is cleaned up.

In all cases, the full transcript and lifecycle event log are retained for audit.

---

## Scenario H — Transcript and Audit Review

After a session ends (or while it is still running), any authorised user can review what the agent did.

### H1. View transcript

From the dashboard, the engineer or manager opens session `S-a83f` and clicks **Transcript**. The full terminal output is displayed — every prompt, response, and command the agent issued.

### H2. View lifecycle events

The session detail view shows an event log:

```
09:14:02  SessionCreated     edge=edge-01  agent=claude  model=claude-3.7-sonnet
09:14:04  WorktreeCreated    path=/data/worktrees/S-a83f
09:14:05  StateChanged       state=running
09:22:18  ClientAttached     mode=ssh  user=jono
10:05:41  ClientDetached     mode=ssh  user=jono
10:06:02  ClientAttached     mode=web  user=jono
10:06:15  ClientAttached     mode=web  user=alex  read_only=true
12:30:00  StateChanged       state=idle
16:30:00  SessionTerminated  reason=idle_timeout
16:30:01  WorktreeRemoved    path=/data/worktrees/S-a83f
```

### H3. Usage accounting

Token usage (input/output, per provider), estimated cost, session duration, and resource consumption are recorded per session and available for cost attribution to engineer, project, or edge node.

---

## What the User Never Sees

The following details are fully abstracted away from the engineer.

| Concern | Mechanism |
|---|---|
| Worktree creation | `git worktree add /data/worktrees/S-a83f` from base repo |
| Credential injection | Sidecar injects provider credentials into session environment; credentials never leave the edge |
| Session creation | `tmux new-session -d -s S-a83f sidecar run claude --repo /data/worktrees/S-a83f` |
| Transcript capture | Sidecar captures PTY output continuously; flushed on detach and terminate |
| Web terminal bridge | `tmux attach -t S-a83f` via PTY-to-WebSocket bridge (xterm.js) with backpressure handling |
| Hub-to-edge routing | Control channel forwarding (direct HTTPS or encrypted outbound tunnel) |
| Policy enforcement | `edged` applies CPU/memory limits, filesystem scope, network access controls, idle timeouts |
| Worktree cleanup | `git worktree remove /data/worktrees/S-a83f` on session destroy |
| Telemetry pipeline | Sidecar → unix socket → `edged` → event stream → control plane → postgres/prometheus |

---

## Summary of User Interfaces

### CLI workflow (Scenarios A–D, G)

| Action | Command |
|---|---|
| Start a session | `edgectl start <agent> --repo <path>` |
| Attach via SSH | `ssh edge-01 -t 'tmux attach -t <session>'` or `edgectl attach <session>` |
| Attach read-only (SSH) | `ssh edge-01 -t 'tmux attach -t <session> -r'` |
| Attach via web | Open `https://edge.company.com/s/<session>` |
| List sessions | `edgectl list` |
| Terminate a session | `edgectl stop <session>` |

### Browser workflow (Scenarios E–H)

| Action | Method |
|---|---|
| Start a session | **New Session** form in web console |
| Attach via web | **Attach (Web)** button → inline terminal |
| Observe read-only | **Observe** button → read-only inline terminal |
| Attach via SSH | **Copy SSH attach** → paste into local terminal |
| List sessions | **Sessions** page / dashboard |
| Terminate a session | **Terminate** button on session card |
| View transcript | **Transcript** tab in session detail |
| View lifecycle events | **Events** tab in session detail |
| Start + attach in one step | **Start & Attach** button (fewer-clicks variant) |

---

## Alignment with Design Principles

| Principle | How These Scenarios Demonstrate It |
|---|---|
| **Terminal-native first** | All scenarios preserve raw PTY fidelity. Agent CLIs run unmodified inside tmux. No proxy layer breaks terminal behaviour. |
| **Edge-native execution** | Sessions run where the data lives. Code and credentials never leave the edge node. Worktrees are local to the edge. |
| **Minimal operator surface** | Engineers use at most three commands (start, list, attach). Browser workflow requires zero CLI. |
| **Observability by default** | Every session produces a transcript and lifecycle event log from the moment it starts. Dashboard metrics are automatic. |
| **Security as a boundary** | Browser access requires authentication. Credentials stay on edge. Read-only attach is sandboxed. Policy enforcement governs resource and filesystem scope. |
