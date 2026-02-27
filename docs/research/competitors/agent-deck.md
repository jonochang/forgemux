# Agent Deck Review vs Forgemux

Date: 2026-02-27

## Scope
This note reviews `/home/jonochang/lib/agent-deck` and compares it with Forgemux (as currently described in `docs/specs/` and `README.md`). It closes with concrete idea seeds for Forgemux.

## Snapshot: Agent Deck
Agent Deck positions itself as a “command center” for many local AI coding sessions. Key traits from the repo:

- TUI-first experience for switching, searching, and managing many concurrent sessions.
- Deep Claude Code integration, including skill management and MCP toggling.
- Multi-session quality-of-life features: search, status icons, notifications, forking.
- Local isolation options: Docker sandboxing; planned Vagrant-based “dangerous” mode.
- Orchestration add-on via “Conductor” sessions (monitor, auto-respond, Slack/Telegram bridge).
- Git worktree workflows as a first-class feature.

## Snapshot: Forgemux
Forgemux is a Rust platform for durable, observable sessions with a tmux-backed edge, optional hub, and a lightweight dashboard. Its roadmap focuses on:

- Phase 0: Durable sessions + accurate state detection (including WaitingInput).
- Phase 1: Notifications on state transitions.
- Phase 2: Hub for multi-node aggregation.
- Phase 3: Browser/mobile attach with reliable stream protocol.

Forgemux emphasizes protocol correctness, state detection, and durability over UI convenience (today).

## Comparison (Agent Deck vs Forgemux)

## Product focus
Agent Deck focuses on a single-user workstation experience with a rich TUI and convenience features. Forgemux focuses on correctness, durability, and multi-node aggregation, and currently exposes minimal UI.

## Session management
Both are tmux-backed and care about session state. Agent Deck invests heavily in in-TUI tools (search, notifications in tmux status line, fork, worktrees). Forgemux is building a state machine and protocol for accurate status detection and multi-node routing.

## Extensibility and integrations
Agent Deck ships in-product skill management and MCP toggling. Forgemux has a broader agent-agnostic focus, with structured logs and sidecar detection as core. Agent Deck’s Conductor is an embedded automation layer; Forgemux has not yet defined a comparable orchestration feature.

## Isolation and safety
Agent Deck includes Docker sandboxing and is designing a Vagrant “dangerous mode”. Forgemux roadmap does not yet describe host isolation or “dangerous mode” toggles, but its architecture is compatible with wrapping session execution.

## UX surface area
Agent Deck is TUI-centric with fast, local workflows. Forgemux is CLI + edge daemon + hub + future dashboard, and expects more distributed environments.

## Idea Seeds for Forgemux
Each idea is tagged with a suggested phase fit or prerequisite.

## 1. Session forking
Why: Agent Deck’s fork workflow makes exploration cheap without losing context. It’s a direct productivity multiplier.
Fit: Phase 0 or Phase 1.
Notes: Forgemux could implement fork as a new session with cloned transcript + initial context seeding. If Claude Code structured logs are available, use them for a faithful fork state.

## 2. First-class worktree support
Why: Agent Deck’s worktree workflow reduces branch collisions and makes parallel agent work safe.
Fit: Phase 0 or Phase 1.
Notes: Forgemux already supports `--worktree` flags; the improvement is automation and cleanup. Add `fmux worktree finish` and `cleanup` analogs for lifecycle management.

## 3. Built-in session search
Why: Agent Deck’s global search (including filters by status) makes high-session-count work viable.
Fit: Phase 1 (watch/notifications) or Phase 2 (hub).
Notes: Provide `fmux search` with status filters, and make it work across hub nodes.

## 4. Status bar notifications for tmux
Why: Agent Deck’s tmux status line notifications are low-latency and align with terminal-native workflows.
Fit: Phase 1.
Notes: Forgemux could emit a concise summary string for tmux status integration or provide a `forged notify --tmux` helper.

## 5. “Conductor” automation layer
Why: Agent Deck’s Conductor is a clear differentiator for managing many sessions. It also enables remote control (Slack/Telegram).
Fit: Phase 2 or Phase 3.
Notes: For Forgemux, consider a “foreman” or “orchestrator” service that subscribes to session state events and can suggest or apply actions. Keep it opt-in and transparent. The foreman could be a first client of the hub event stream.

## 6. MCP and skill pool management
Why: Agent Deck’s pool-based skills and MCP toggles reduce config drift across many repos.
Fit: Phase 1 or Phase 2.
Notes: Forgemux could formalize agent-specific attachments via a “session attachments” concept. Keep it agent-agnostic: attachments map to CLI flags, env vars, or mounted config directories.

## 7. Isolation options as execution wrappers
Why: Agent Deck’s Docker sandbox and the proposed Vagrant “dangerous mode” are practical guardrails.
Fit: Phase 1 or Phase 2.
Notes: Forgemux can wrap session start commands with a “runner” abstraction: local, docker, vm. This would align with the existing sidecar model and allow controlled execution without changing the core state machine.

## 8. Fast session switch UX
Why: The TUI “jump between any session with a keystroke” is a strong daily-use feature.
Fit: Phase 1 or Phase 2.
Notes: Forgemux could add a lightweight TUI frontend or a `fmux switch` that uses fzf-like selection. This does not require a full UI overhaul.

## 9. Reliable reconnection workflow
Why: Agent Deck’s focus on resilience (MCP pooling auto-reconnect, status recovery) fits Forgemux’s durability goals.
Fit: Phase 3.
Notes: Use this as a north star for the browser attach protocol. Ensure reconnection and replay feel as seamless as local tmux.

## 10. “Skill-based docs” for quick LLM onboarding
Why: Agent Deck ships its own “skill” docs and in-repo llms.txt for easy agent help.
Fit: Phase 0 or Phase 1.
Notes: Provide a `llms.txt` or skill doc in Forgemux, focused on CLI usage, config, and state machine behavior. This also improves support for AI-driven ops.

## Strategic takeaways
- Agent Deck’s primary strength is fast, local workflow and per-session ergonomics.
- Forgemux’s core differentiator should remain correctness and durability of sessions and state, but adding a thin layer of ergonomics (forking, search, worktrees, tmux status) would materially improve usability without compromising architecture.
- The “Conductor” concept maps naturally onto Forgemux’s hub-centric phase 2+ design, and could be a compelling feature once the event stream is stable.

## Proposed next steps (low effort)
- Add a `docs/research` follow-up to scope `fmux worktree finish` + `cleanup` and `fmux fork` semantics.
- Draft a minimal `fmux search` UX that works with current edge APIs.
- Add a short `llms.txt` or skill doc to help LLMs operate Forgemux consistently.
