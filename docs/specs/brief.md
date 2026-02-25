# Product Briefing: Forgemux

**Document type:** Product Briefing
**Author:** Silverpond Engineering
**Date:** February 2026
**Status:** Draft

---

## Executive Summary

The Forgemux is an internal platform that transforms how Silverpond engineers run, monitor, and manage AI coding agents. It wraps agent CLIs — such as Claude Code and Codex CLI — in a durable, observable, enterprise-grade session layer that runs at the edge, without sacrificing the native terminal experience engineers already rely on.

In short: engineers keep their terminal workflow. The organisation gains visibility, control, and auditability over every agent session.

---

## The Problem

Today, engineers run AI coding agents interactively inside local terminals. These sessions are stateful, long-lived, and tied to local filesystems — but they are also fragile, invisible, and unmanaged.

**For the engineer**, a dropped SSH connection or a closed laptop means losing an active agent session and its accumulated context. There is no way to hand off a running session to a colleague, resume work from a browser, or review what an agent did while unattended.

**For the organisation**, there is no central view of which agents are running, what they are working on, how much they cost, or what actions they have taken. Sensitive code and data sit on edge nodes with no audit trail. There is no mechanism to enforce resource limits or usage policies across agent sessions.

**For the team lead**, sharing a live agent session for review or pair-debugging requires ad-hoc screen sharing. There is no way to observe an agent's progress without interrupting the engineer running it.

These gaps widen as agent usage scales. What works for one engineer running one agent becomes untenable when a team runs dozens of concurrent agent sessions across multiple edge nodes.

---

## The Solution

The Forgemux provides a thin, unified management layer over agent CLI sessions. Every agent runs inside a tmux-backed session on the edge node where the code and data reside. These sessions become first-class, durable objects that can be started, attached, detached, resumed, observed, and terminated — from either a terminal or a browser.

### Core Capabilities

**Durable sessions.** Agent sessions survive network disconnects, closed terminals, and laptop sleeps. An engineer can start a session, walk away, and reattach hours or days later with full context preserved.

**Dual access modes.** Sessions can be attached via native SSH (`tmux attach`) or via a browser-based terminal over WebSocket. Both modes operate on the same underlying tmux session and can coexist simultaneously — an engineer can work via SSH while a reviewer watches from the browser.

**Real-time observability.** A central dashboard shows all active sessions across edge nodes, including agent type, model, session state (running, idle, waiting for input, error), token usage, estimated cost, resource consumption, attached users, and last activity timestamp.

**Full audit trail.** Every session produces a retained transcript, a lifecycle event log (start, attach, detach, terminate), and aggregated token usage records. These support compliance, cost attribution, and post-hoc review of agent behaviour.

**Policy enforcement.** Sessions can be governed by per-session or per-node policies covering CPU and memory limits, filesystem scope restrictions, network access controls, API rate limits, and idle timeouts.

**Zero-friction start.** Engineers start a session with a single CLI command or a single browser workflow. Sensible defaults — auto-detected repository, preset model configuration — eliminate setup friction.

### The Foreman Agent

As Forgemux manages more concurrent sessions, a new problem emerges: engineers can't keep track of what all their agents are doing. The Foreman agent addresses this by acting as a meta-agent — a supervisor that watches other sessions and provides situational awareness, stall detection, and orchestration.

**The key architectural insight: the Foreman is itself a Forgemux session.** It runs Claude Code or Codex CLI in a managed tmux session, just like any work agent. It does not call LLM APIs directly. Instead, it reads other sessions' transcripts and state through forged's internal APIs, reasons about them using the same agent CLIs engineers use, and reports its findings back through the standard session substrate.

This means:

- No new infrastructure. The Foreman is managed by the same lifecycle, observability, and policy enforcement as every other session.
- No API key management beyond what already exists. The Foreman uses the same agent CLIs with the same credentials.
- Full auditability. The Foreman's reasoning is captured in its own transcript, reviewable like any other session.
- Natural cost control. The Foreman's own token usage is tracked and bounded by the same policies.

**What the Foreman provides:**

**Session situation awareness.** The Foreman periodically reads active session transcripts and produces structured summaries: what each agent is working on, which files it has touched, what its current hypothesis appears to be, and whether it looks productive, blocked, or looping.

**Stall detection.** The Foreman identifies sessions that are stuck — repeated errors, rebuilding the same file, high token usage with no file diffs, or circular reasoning. It flags these with a diagnosis and suggested intervention.

**Auto-summarisation.** When a session exceeds a token or time threshold, the Foreman generates an executive summary: key files touched, commands executed, open questions, and suggested next steps. Engineers read a paragraph instead of scrolling through thousands of lines.

**Intervention modes.** The Foreman operates at three levels, selectable by policy:

- *Advisory* (default): suggests actions in its own session panel; engineer reviews and acts manually.
- *Assisted*: proposes exact commands; engineer approves with one click and the command is injected into the target session's tmux input.
- *Autonomous* (opt-in): the Foreman can spawn helper sessions, run parallel diagnostics, and post findings back to the original session — all within Forgemux's managed substrate.

**Specialist session spawning.** When the Foreman detects a problem it can decompose (e.g., a complex compiler error), it can spawn a separate agent session to analyse the issue in isolation, then summarise the result and feed it back. This is parallel multi-agent work without the engineer orchestrating it manually.

**Budget awareness.** The Foreman monitors token burn rates across sessions and can warn when a session approaches a cost threshold, or recommend model downgrades when policy allows.

---

## How It Works

### Session Lifecycle

```
Start → Running → Idle → Waiting for Input → Running → ... → Terminated
          ↕                                      ↕
       Detached ←──────────────────────────── Detached
          ↕                                      ↕
       Reattached                            Reattached
```

1. **Start.** Engineer issues a single command (CLI) or clicks "New Session" (browser). The orchestrator creates a tmux session on the target edge node, injects credentials, and launches the agent CLI.
2. **Run.** The agent operates normally inside the tmux session. A lightweight sidecar monitors session state, captures terminal output for the transcript, and reports metrics to the control plane.
3. **Attach / Detach.** Engineers attach and detach freely. Multiple concurrent viewers are supported. SSH and browser attach modes can be used interchangeably or simultaneously.
4. **Terminate.** Sessions can be terminated by the engineer, by an idle timeout policy, or remotely by an administrator via the dashboard.

### Architecture at a Glance

| Layer | Role |
|---|---|
| **Agent CLI** | Claude Code, Codex CLI, or other TUI-based agents — unmodified |
| **tmux session** | Durable session substrate; preserves raw PTY fidelity |
| **Session sidecar** | Lightweight process monitoring state, capturing transcripts, reporting metrics |
| **Edge daemon** | Manages session lifecycle on a single edge node; enforces local policy |
| **Foreman agent** | Meta-agent session that supervises other sessions — runs as a standard Forgemux session using agent CLIs |
| **WebSocket bridge** | Translates tmux PTY I/O to xterm.js in the browser; handles backpressure |
| **Control plane API** | Aggregates session state across nodes; serves dashboard and management endpoints |
| **Dashboard** | Browser-based console for observation, management, and session attach |

---

## Design Principles

**Terminal-native first.** The platform must never degrade the native CLI experience. Agent CLIs are full-screen interactive TUIs that depend on raw PTY behaviour. No proxy or abstraction layer may break terminal fidelity.

**Edge-native execution.** Sessions run where the data lives. Code and sensitive data never leave the edge node unless explicitly permitted. The platform must function in LAN-only, VPN, or NAT-traversed network environments.

**Minimal operator surface.** The system exposes a small number of user-facing commands. Engineers should not need to understand tmux, manage sessions manually, or re-authenticate agents between sessions.

**Observability by default.** Every session is observable from the moment it starts. Metrics, state, and transcripts are collected automatically — engineers opt out, not in.

**Security as a boundary.** Browser access is authenticated and cannot bypass SSH privilege boundaries. Credentials remain on the edge. Agent sessions can be sandboxed by filesystem, network, and resource policy.

---

## Target Users

| Role | Primary Value |
|---|---|
| **Software engineer** | Durable, resumable agent sessions; no lost context; browser access when SSH is inconvenient |
| **Tech lead / reviewer** | Live observation of agent sessions; read-only attach for review without interruption |
| **Engineering manager** | Dashboard view of team agent activity; cost visibility; usage trends |
| **Platform / DevOps** | Centralised session management; policy enforcement; audit compliance |
| **Security** | Full transcript and event logging; credential isolation; bounded execution environments |

---

## Key Metrics

**Operational metrics** tracked per session and aggregated across nodes:

- Token usage (input/output, per provider) and estimated cost
- Session duration and active time
- CPU and memory consumption
- Agent state distribution over time
- Attach/detach frequency by access mode (SSH vs. browser)

**Platform health metrics:**

- Session loss rate (target: near zero)
- Web attach latency overhead (target: < 50ms)
- Sidecar CPU overhead (target: negligible)
- Concurrent sessions per edge node

---

## Constraints and Boundaries

**In scope:** Session lifecycle management, dual-mode attach, observability, transcript capture, policy enforcement, usage accounting, multi-node aggregation, Foreman meta-agent for session supervision and orchestration.

**Out of scope (for initial release):** Automated task scheduling and queuing, custom agent CLI development, CI/CD pipeline integration, multi-tenancy across organisational boundaries.

**Technical constraints:**

- tmux is the mandatory session substrate — no alternative session managers
- Agent CLIs must be run unmodified; no source-level instrumentation
- Token usage accounting prefers provider-native reporting; falls back to estimation only when necessary
- All edge-to-hub communication must be encrypted and authenticated
- The platform must operate in offline or semi-isolated network environments

---

## Security Model (Summary)

| Concern | Approach |
|---|---|
| Credential exposure | All credentials remain on edge; never transmitted to dashboard or browser |
| Browser session escape | Browser attach is sandboxed to the tmux session; no shell escape |
| Authentication | Browser clients authenticate before attach; SSH uses existing key infrastructure |
| Network isolation | Agents can be restricted to specific network scopes per policy |
| Filesystem scope | Agent sessions can be confined to specific directory trees |
| Audit | All lifecycle events, attaches, and transcripts are logged immutably |

---

## Strategic Value

This product transforms the current state of affairs — individual engineers running private, invisible agent sessions in local terminals — into a managed, observable, secure, and resumable computation layer at the edge. It does so without removing the terminal-native developer experience that makes these agents productive in the first place.

The Foreman agent extends this further: by supervising agent sessions using the same agent CLIs, Forgemux enables a team to run agents at a scale that would otherwise require constant human monitoring. An engineer can start multiple agents, let the Foreman watch them, and only intervene when needed — shifting from operator to reviewer.

As agent-assisted development scales across the team, this platform provides the operational backbone required to run agents responsibly: with visibility into what they are doing, control over how they operate, and accountability for what they have done.

---

## Open Questions for Discussion

1. **Session sharing model.** Should sessions be shareable by default (read-only), or require explicit grants? What permission model fits the team's workflow?
2. **Transcript retention policy.** How long should full transcripts be retained? Should they be searchable, or archival only?
3. **Cost attribution.** Should token costs be attributed to the engineer, the project, or the edge node? How granular does cost reporting need to be?
4. **Agent autonomy policy.** Under what conditions should an agent session be allowed to run unattended indefinitely? What guardrails are appropriate?
5. **Multi-node topology.** For the initial release, is a single control plane with multiple edge nodes sufficient, or is federated/mesh topology required?
6. **Foreman intervention level.** What should the default intervention mode be? Should autonomous mode (spawning helper sessions, injecting commands) require per-session opt-in, or can it be set as a node-wide policy?
7. **Foreman cost budget.** The Foreman consumes tokens to supervise other sessions. What is an acceptable overhead ratio (e.g., Foreman cost as % of supervised session cost)? Should the Foreman self-throttle when it detects diminishing returns?

---

*This document is intended for internal circulation within Silverpond engineering leadership. It defines the product vision, scope, and constraints to align stakeholders before detailed design begins.*
