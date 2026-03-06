# OpenFang Review vs Forgemux

Date: 2026-03-02

## Scope
This note reviews `/home/jonochang/lib/openfang` and compares it with Forgemux (as described in `docs/specs/` and `README.md`). It closes with idea seeds for Forgemux.

## Snapshot: OpenFang
OpenFang positions itself as an “Agent Operating System” built in Rust. The repo emphasizes an all-in-one binary, extensive security hardening, bundled autonomous agents (“Hands”), and a large surface area across channels, providers, tools, and UI.

Key claims and traits from the repo:
- One ~32MB binary, Rust workspace with 14 crates and a large test suite.
- A full stack: kernel, runtime, memory, API, CLI, TUI, desktop app (Tauri), and a web dashboard.
- “Hands”: bundled autonomous agents that run on schedules without prompts.
- A workflow engine for multi-step agent pipelines, plus triggers and scheduler.
- Heavy security posture with 16 distinct mechanisms (WASM sandbox, audit hash chain, capability gates, SSRF protection, etc.).
- A broad integration matrix: 40 channel adapters, 27 LLM providers, 50+ tools.
- Multi-protocol support: MCP, A2A, and a custom P2P wire protocol (OFP).

## Snapshot: Forgemux
Forgemux is a Rust platform for durable, observable sessions with a tmux-backed edge, optional hub, and a lightweight dashboard. The roadmap centers on correct state detection, reliable session durability, and scalable multi-node aggregation, with UI kept intentionally minimal today.

## Comparison (OpenFang vs Forgemux)

## Product focus
OpenFang is an end-to-end agent OS with a user-facing desktop + web UX and a large built-in surface area (agents, channels, providers, marketplace). Forgemux is a durable session substrate and control plane for tool-driven sessions, with a smaller product surface and a tighter focus on correctness and durability.

## Session model
OpenFang manages agents as first-class objects with manifests, tools, capabilities, workflows, triggers, and scheduling. Forgemux treats sessions as durable execution contexts (tmux-backed) with explicit states and robust attach/detach semantics. Forgemux’s session state model is narrower but aims for higher reliability in terminal-native workflows.

## Architecture depth
OpenFang is a vertically integrated platform: kernel boot sequence, agent lifecycle, memory substrate, multi-provider LLM routing, channels, skills, and a custom wire protocol. Forgemux is a horizontal substrate: a state machine, edge daemon, optional hub, and an API/event stream to enable higher-level orchestration elsewhere.

## Security posture
OpenFang invests heavily in layered security (WASM sandbox with dual metering, Merkle audit trail, taint tracking, capability gates, SSRF protection, prompt injection scanner, etc.). Forgemux focuses on protocol correctness and session durability, but does not advertise a comparable security stack today. If Forgemux becomes a general execution substrate, OpenFang’s security posture will be a competitive benchmark.

## Distribution and UX
OpenFang ships with a TUI dashboard, desktop app, and web UI out of the box. Forgemux expects CLI-first usage and later a lightweight dashboard. OpenFang’s UX is larger in scope, while Forgemux’s strength is its minimal, dependable core.

## Extensibility and integrations
OpenFang has a marketplace and a skill system, plus MCP/A2A compatibility. Forgemux is agent-agnostic and can integrate via sidecars, structured logs, and session attachments, but does not provide a bundled marketplace or skill lifecycle in-repo.

## Scale and multi-node
OpenFang uses a P2P protocol (OFP) for peer networking. Forgemux plans a hub for aggregation and multi-node routing. The approaches are orthogonal: OpenFang emphasizes peer-to-peer agent connectivity, while Forgemux emphasizes centralized observability and routing.

## Idea Seeds for Forgemux
Each idea is tagged with a suggested phase fit or prerequisite.

## 1. Security tiering for sessions
Why: OpenFang’s layered security messaging is a major differentiator for risk-averse users. Forgemux could offer a minimal but explicit security story.
Fit: Phase 1 or Phase 2.
Notes: Introduce session “security profiles” that wrap a session with optional sandboxing (Docker/VM), environment scrubbing, and network allowlists. Start with a simple runner abstraction to match OpenFang’s security posture without copying its full stack.

## 2. Workflow/trigger primitives on top of session events
Why: OpenFang’s workflows and triggers provide a clear automation story. Forgemux already has a state/event model; exposing this for automation would be low-friction.
Fit: Phase 1 or Phase 2.
Notes: Provide `fmux workflow` or `fmux trigger` that binds to session events and can spawn or route tasks. Keep the core lean by making it event-stream driven and externalizable.

## 3. “Hands” as a packaging pattern
Why: OpenFang’s “Hands” bundling (manifest + system prompt + skill docs + guardrails) is a strong narrative for packaged autonomous behaviors.
Fit: Phase 0 or Phase 1.
Notes: Forgemux can define a minimal “session bundle” format that maps to tmux sessions, entry commands, and attached docs. This keeps the narrative without building a full marketplace.

## 4. Capability and approval gates
Why: OpenFang’s approval gates (e.g., purchases) are concrete trust builders. Forgemux can supply a similar guardrail at the session layer.
Fit: Phase 1.
Notes: Add a “requires approval” state that blocks specific actions until a human approves, using the existing state machine and notification hooks.

## 5. TUI quick dashboard
Why: OpenFang’s TUI lowers friction for local workflows. Forgemux could gain a lot with a very thin terminal UI.
Fit: Phase 1.
Notes: A minimal ratatui-based session list, status view, and attach shortcut would cover 80% of the value without building a full web UI.

## 6. Web UI and desktop pairing
Why: OpenFang’s desktop app is a strong “one install” story. Forgemux could reach similar users with a lightweight desktop wrapper around the dashboard.
Fit: Phase 3.
Notes: Use the hub web UI as the base, then add a Tauri wrapper after the protocol is stable.

## 7. Multi-provider abstraction at the hub
Why: OpenFang’s provider matrix and routing are a differentiator. Forgemux can integrate with existing agent runtimes but may need a clear story for upstream LLM selection.
Fit: Phase 2 or Phase 3.
Notes: Offer a “provider profile” abstraction at the hub so sessions can declare LLM requirements and the hub can choose a provider or route to a sidecar.

## 8. Auditability and replay
Why: OpenFang’s Merkle audit trail and session repair emphasize integrity. Forgemux already cares about durable session state.
Fit: Phase 1 or Phase 2.
Notes: Add an append-only event log with optional hashing. Even if simplified, it gives concrete audit and replay semantics and aligns with Forgemux’s durability brand.

## Strategic takeaways
- OpenFang’s value proposition is breadth: a complete agent OS with security, UI, and packaged autonomous behaviors.
- Forgemux’s advantage is focus: durable, correct sessions with a clean event model and scalable topology.
- The biggest competitive risk is perception: OpenFang looks like a full product while Forgemux can appear “infrastructure-only.” A few thin UX and security affordances could shift that perception without bloating the core.

## Proposed next steps (low effort)
- Draft a short security positioning note for Forgemux that explains execution boundaries and possible sandbox options.
- Define a minimal “session bundle” spec that resembles a “Hand” but maps to tmux sessions and attachments.
- Sketch a minimal TUI session list (status + attach + search) as a Phase 1 UX layer.
