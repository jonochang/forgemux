# Forgemux Competitive Landscape Report

Date: 2026-02-27
Sources: 12 deep-dive research documents across 6 competitors (docs/research/competitors/*)

---

## 1. Executive Summary

The AI coding agent orchestration market is rapidly consolidating around a common product shape: durable session management + observability + lightweight task semantics + cross-device control. Six competitors were analyzed across two rounds each of source-code-level review:

| Competitor | Architecture | Primary Moat |
|---|---|---|
| **Agent Deck** | Go TUI + tmux + SQLite | Single-user ergonomics, fast iteration (releases every 1-2 days) |
| **Superset** | TypeScript monorepo, Electron + Postgres + Electric SQL | Full SaaS stack, multi-device sync, tRPC type safety |
| **Happy Coder** | TypeScript, React Native + Fastify + E2E encryption | Mobile-first remote control, zero-knowledge server |
| **claudedash** | TypeScript, local observer | Observability depth: snapshots, context health, quality gates |
| **Mission Control** | Next.js + JSON files + daemon | Task management depth: Eisenhower, Kanban, decisions, scheduling |
| **Beehive** | Tauri (Rust) desktop app + direct PTY | Desktop GUI polish, zero-config onboarding |

**Market position:** The market is bifurcating into two tiers:
- **Desktop-first tools** (Agent Deck, Beehive, Superset) that optimize for single-developer ergonomics
- **Infrastructure-first tools** (Forgemux, Happy Coder, Mission Control) that optimize for durability, multi-node, and programmatic control

Forgemux sits at the intersection: infrastructure-grade durability with an architecture that can serve both tiers. The strategic question is not which tier to choose, but how to layer ergonomics on top of infrastructure without diluting the core.

---

## 2. Common Themes

These patterns appeared in 4+ of the 6 competitors analyzed.

### 2.1 Session Durability Is Table Stakes
Every competitor addresses session persistence in some form. tmux-backed sessions (Forgemux, Agent Deck), persistent PTYs with state files (Beehive, Superset), daemon lifecycle management (Happy), or task-based recovery (Mission Control, claudedash). The minimum expectation is that a session survives a network drop and can be resumed. The emerging expectation is that sessions survive machine reboots and can be replayed.

### 2.2 Observability Is Moving Beyond Raw Logs
Raw terminal output is no longer sufficient. Competitors are layering structured signals on top:
- **Context health** (claudedash): token usage as a percentage of context window, with warning thresholds
- **Quality gates** (claudedash): lint/test/typecheck results surfaced per task
- **Structured events** (Happy): typed message envelopes with role, tool call, and turn boundaries
- **Insights engines** (claudedash): velocity, bottlenecks, success rate computed from event logs
- **Cost tracking** (Superset, Happy): token-to-dollar mapping as a first-class dashboard metric

The common direction: observability is becoming **semantic** (what is the agent doing and how well) rather than **syntactic** (what bytes did the terminal emit).

### 2.3 The Supervisor Agent Pattern Is Converging
Three competitors have shipped or are building a meta-agent that watches other agents:
- **Agent Deck's Conductor**: persistent Claude Code session with heartbeat monitoring, context management, permission loop fixes, Telegram/Slack bridge
- **Mission Control's Daemon**: scheduler + dispatcher + runner + health monitor pipeline
- **Forgemux's Foreman** (spec): three intervention levels, hub integration

Agent Deck has iterated furthest and surfaced real operational problems: context window overflow in the supervisor, permission loops where the supervisor triggers its own approval prompts, and crash recovery for the supervisor itself. These are problems Forgemux's Foreman spec should address before shipping.

### 2.4 Worktree Isolation Is Mandatory for Parallel Work
Every competitor that supports parallel sessions has landed on git worktrees as the isolation primitive:
- Agent Deck: full worktree lifecycle (create, cleanup, orphan detection)
- Superset: per-task worktree with branch isolation
- Beehive: workspace cloning (full directory copy, heavier than worktrees)
- Forgemux: `--worktree` flag exists but no lifecycle management

The gap: worktree creation is easy; the hard part is lifecycle management (cleanup, orphan detection, branch divergence tracking). Agent Deck has the most mature implementation.

### 2.5 Cross-Device Control Is Becoming Core
The assumption that an engineer is sitting at the same terminal as their agent is fading:
- Happy Coder: mobile-first, push notifications, QR pairing
- Superset: device presence + command routing across machines
- Agent Deck: web mode + PWA + web push + Telegram/Slack bridge
- claudedash: SSE dashboard with billing window awareness

The pattern: a lightweight web/mobile surface for monitoring and triage, with full terminal attach reserved for deep intervention.

### 2.6 Typed Protocols for Survival
Agent CLI output formats change frequently. Competitors are defending against this:
- Happy Coder: Zod-validated wire protocol with versioned envelopes
- Superset: tRPC end-to-end type safety
- claudedash: explicit compatibility contract documenting watched JSONL fields
- Agent Deck: per-tool pattern matching with tool-specific adapters

The fragility of regex-based state detection is a known risk across the market. The emerging answer is a combination of structured protocols (where possible) and compatibility contracts (where not).

### 2.7 Security and Trust Models Are Diverging
- **Happy Coder**: zero-knowledge server, E2E encryption, client-side crypto
- **Mission Control**: credential scrubbing (14 regex patterns), prompt injection fencing, binary whitelist, safe environment construction
- **Forgemux**: bearer tokens + optional AES-256-GCM stream encryption
- **Agent Deck, Beehive, Superset**: minimal security (local-only assumption or basic auth)

The market hasn't settled on a security model. For infrastructure-grade tools, the Mission Control + Happy approach (scrub credentials, fence prompts, encrypt at rest) sets the standard.

---

## 3. Market Dimensions

These are the axes along which products compete and buyers differentiate.

### 3.1 Session Substrate
| Approach | Products | Tradeoffs |
|---|---|---|
| tmux (external process) | Forgemux, Agent Deck | Durable, multi-client attach, but requires tmux and shell-based interaction |
| Direct PTY (in-process) | Beehive, Superset | Lower latency, no external deps, but sessions die with the host process |
| Fire-and-forget CLI spawn | Mission Control | Simplest, but no observation or interaction during execution |

**Winner:** tmux for infrastructure; direct PTY for desktop apps. The hybrid approach (direct PTY for local dashboard, tmux for remote/durable) is unexplored territory.

### 3.2 State Model Sophistication
| Approach | Products | Tradeoffs |
|---|---|---|
| Explicit FSM + OCC | Forgemux (7-state FSM, versioned records) | Precise lifecycle guarantees, harder to implement |
| Implicit UI-driven | Superset, Beehive | Simpler, but state is scattered across UI components |
| Polling + heuristics | Agent Deck, Mission Control | Practical but fragile across agent CLI updates |
| Structured log watching | Happy, claudedash | Higher fidelity than PTY regex, depends on agent log format stability |

**Forgemux advantage:** The explicit FSM with optimistic concurrency is the most rigorous model in the market. No competitor matches this.

### 3.3 Task Semantics
| Approach | Products | Tradeoffs |
|---|---|---|
| Full task management | Mission Control | Rich but creates a product-layer dependency |
| Thin work item / plan overlay | claudedash, Superset | Bridges task-to-session without becoming a PM tool |
| Session-only | Forgemux, Agent Deck, Beehive, Happy | Clean but forces external task management |

**Market direction:** "Thin task overlay" is winning. Full task management is too opinionated; session-only is too bare. The sweet spot is a WorkItem that links intent to sessions without owning the backlog.

### 3.4 Deployment Topology
| Approach | Products | Tradeoffs |
|---|---|---|
| Single-node desktop | Beehive, Superset (Electron), Agent Deck | Zero setup, but locked to one machine |
| Single-node headless | Mission Control, claudedash | CLI-operable, but no multi-machine |
| Multi-node edge/hub | Forgemux, Happy (server relay) | Scales, but more complex to deploy |

**Forgemux advantage:** The edge/hub topology is architecturally unique in this market. Superset's device presence is the closest competitor, but it routes through a centralized cloud, not a federated topology.

### 3.5 Integration Surface
| Integration | Products |
|---|---|
| GitHub (PRs, issues, webhooks) | Superset, Mission Control |
| Slack/Telegram | Agent Deck (conductor bridge), Superset |
| Linear | Superset |
| MCP (agent self-introspection) | claudedash, Agent Deck, Superset |
| Webhook (outbound) | Happy, Forgemux (planned) |
| Claude Code hooks | claudedash, Agent Deck |

**Gap for Forgemux:** No first-party integrations shipped. The webhook system is planned but not implemented. Event-driven session creation (GitHub issue assigned -> session starts) is the step change that turns Forgemux from a session manager into an orchestration platform.

### 3.6 Security Posture
| Capability | Products |
|---|---|
| E2E encryption | Happy Coder, Forgemux (stream-level) |
| Credential scrubbing | Mission Control |
| Prompt injection fencing | Mission Control |
| Container isolation | Agent Deck (Docker), Forgemux (cgroups planned) |
| Binary whitelist / safe env | Mission Control |
| Auth (OAuth, RBAC) | Superset |

---

## 4. Market Forecast

### 4.1 Near-Term (6 months)
- **Supervisor agents become standard.** Agent Deck's Conductor is already shipping. Expect every "command center" to have a meta-agent within 6 months. The differentiation will be in context management, failure recovery, and cost efficiency of the supervisor.
- **Mobile/web control becomes expected.** The "fire and check from your phone" UX that Happy pioneered will be copied. Push notifications on WaitingInput will be table stakes.
- **Worktree lifecycle management matures.** Cleanup, orphan detection, and branch divergence tracking will move from "nice to have" to "required."

### 4.2 Medium-Term (6-12 months)
- **Event-driven session creation.** Today, sessions are manually started. The next wave is automated: GitHub webhook -> session starts -> agent works -> PR opened -> session terminates. Superset's webhook handlers hint at this direction.
- **Agent protocol standardization pressure.** As more tools wrap Claude Code, Codex, Gemini, etc., there will be pressure for agents to emit structured lifecycle events rather than relying on PTY output parsing. Tools that define and promote an agent event protocol gain influence.
- **Cost-aware orchestration.** As agent usage scales, tools that can throttle, batch, and budget session creation based on token spend and billing windows will become critical. claudedash's billing window awareness is the first signal of this.

### 4.3 Long-Term (12-24 months)
- **Multi-agent collaboration protocols.** Beyond supervisor-worker, expect peer-to-peer coordination: agents that discover each other's work, avoid conflicts, and merge results. The MCP self-introspection pattern (claudedash, Agent Deck) is the foundation.
- **Verifiable agent outputs.** As agents do more unsupervised work, the demand for provable correctness (tests passed, no regressions, security checks green) will drive "session contracts" that define and verify success criteria.
- **Fleet management at scale.** Organizations running 50-100+ concurrent agent sessions will need fleet-level primitives: quotas, priority queues, load balancing across edges, and centralized audit. This is where Forgemux's edge/hub topology becomes a structural advantage.

---

## 5. Strategic Recommendations

### 5.1 What to Build (Priority Order)

**P0 - Foundation (ship first)**

1. **Credential scrubbing in transcripts and logs.** Mission Control's 14-pattern scrubber is proven. Low effort, high trust signal. Apply to transcripts before disk write, API responses, and notification payloads.

2. **Worktree lifecycle management.** `fmux start --worktree feature-x` creates worktree; `fmux stop --cleanup-worktree` removes it; `fmux worktree list` shows worktrees with associated sessions; `fmux worktree cleanup` finds orphans. Tie worktree lifecycle to session lifecycle.

3. **Session forking via agent-native resume.** Read Claude's conversation ID from JSONL logs. `fmux fork S-xxxx` creates a new session with `--resume <conversation-id>`, optionally in a new worktree. Store `agent_session_id` in SessionRecord.

**P1 - Ergonomics (ship second)**

4. **Session templates.** TOML-defined templates with agent, model, policy, env vars, setup scripts. `fmux start --template review`. Templates live in `forgemux.toml` (global) and `.forgemux.toml` (per-repo).

5. **Event-streamed dashboard.** SSE or WebSocket endpoint that pushes session state changes. "Needs attention" view: WaitingInput, Errored, high context pressure. Replaces polling.

6. **Context health as a first-class signal.** Parse `stats-cache.json` and usage JSONL. Surface context pressure (% of window used) as a badge on session cards. Warn at 65%/75%.

7. **Outbound webhooks.** POST session state changes to configurable URLs. Enable Slack/Discord/custom integrations without building first-party bridges. HMAC-signed payloads.

**P2 - Differentiation (ship third)**

8. **Agent self-introspection via MCP.** `forgemux-mcp` binary (stdio transport) wrapping the forged REST API. Tools: `list_sessions`, `get_status`, `get_usage`, `get_worktree_status`. Enables Foreman and cooperative agents.

9. **Structured session protocol.** Typed event envelopes with durability flags (durable vs ephemeral). Ring buffer stores only durable events. Replay on WebSocket connect skips ephemeral. Versioned for forward compatibility.

10. **Snapshot + recovery system.** Snapshot = session record + transcript tail + git state + worktree diff. Keyed by commit hash. `fmux snapshot S-xxxx` to capture; `fmux recover S-xxxx` to inspect and optionally reset. Foreman auto-snapshots before interventions.

### 5.2 What Not to Build

- **Full task management.** Leave Eisenhower matrices, Kanban boards, and goal hierarchies to Mission Control, Linear, and Jira. Forgemux should consume tasks (via WorkItems or webhooks), not manage them.
- **Desktop GUI.** Beehive and Superset own this space with Tauri/Electron. Forgemux should invest in a thin TUI (`ratatui`) and a web dashboard, not a native app.
- **Agent-specific UI features.** Skills management, MCP toggling, profile switching. These are agent-level concerns. Forgemux should support injecting config at session start, not building management UIs for each agent's settings.

---

## 6. Out-of-Distribution Ideas

These are intentionally non-obvious. The goal is differentiation through capabilities no competitor is building.

### 6.1 Session Contracts with Automated Verification
Define a machine-readable "contract" for each session: required artifacts, tests that must pass, files that must exist, and forbidden patterns (e.g., no `console.log` in production code).

```toml
[contracts.feature-work]
required_files = ["src/**/*.test.ts"]
required_commands = ["npm test"]  # must exit 0
forbidden_patterns = ["console.log", "TODO: remove"]
max_duration_minutes = 120
```

When a session completes, forged runs the contract checks and marks the session as `Verified`, `Partial`, or `Failed`. The Foreman can auto-retry failed contracts. The dashboard shows verification status alongside session state.

**Why it's OOD:** No competitor verifies agent outputs. Everyone trusts the agent's self-report. Automated verification turns Forgemux into a CI-like system for agent work.

### 6.2 Attention Budgeting Engine
Model human attention as a finite resource. Each engineer has a configurable daily attention budget (e.g., 10 interruptions/day). The Foreman classifies events by severity and only interrupts when the budget allows.

- Critical (deploy failure, security issue): always interrupt
- High (WaitingInput on important session): interrupt if budget > 3
- Medium (session completed, needs review): batch into hourly digest
- Low (usage report, routine completion): daily summary only

The budget resets daily. The dashboard shows remaining budget and queued events. Engineers stop getting notification fatigue; Forgemux becomes the system that respects their time.

**Why it's OOD:** Every competitor sends notifications on every state change. No one models the human's capacity to absorb them.

### 6.3 Causality Graph for Agent Actions
Build a directed acyclic graph linking: prompt -> tool calls -> file modifications -> test results. Stored per session, queryable via API.

```
User prompt: "Add auth middleware"
  -> tool_call: bash("npm install passport")
  -> file_write: src/middleware/auth.ts (created)
  -> file_write: src/routes/api.ts (modified, lines 45-52)
  -> tool_call: bash("npm test")
  -> test_result: 14 passed, 0 failed
```

This enables:
- "Why did this file change?" queries without reading the full transcript
- Blame attribution for agent-introduced bugs
- Foreman decision-making based on action patterns (e.g., "this agent has modified 30 files without running tests -- intervene")

**Why it's OOD:** Everyone stores transcripts as linear text. No one builds a queryable action graph.

### 6.4 Session Risk Scoring
Compute a real-time risk score per session based on signals:
- Context pressure (high = risky, agent may lose coherence)
- Edit velocity (many rapid edits without tests = risky)
- Test failure rate (increasing failures = risky)
- Token burn rate (accelerating spend = possible loop)
- File scope (touching many unrelated files = risky)

Score: 0-100. Dashboard shows a risk heatmap. Foreman prioritizes high-risk sessions for supervision. Engineers review high-risk sessions before merging.

**Why it's OOD:** Competitors show binary states (running/idle/error). No one quantifies risk as a continuous signal.

### 6.5 Zero-Install Session Attach
Generate a one-time, time-limited URL that lets anyone view a session in read-only mode in their browser. No install, no auth setup, no CLI.

Use case: an engineer sends a link to a PR reviewer, security auditor, or team lead. They click it and see the agent's terminal output, current file changes, and session state. The link expires after 1 hour or one use.

Implementation: forgehub generates a signed JWT embedded in the URL. The dashboard renders a read-only xterm.js view. No write access, no send-keys.

**Why it's OOD:** Every competitor requires the viewer to install the tool or have an account. Frictionless sharing of agent work is unexplored.

### 6.6 Federated Privacy Mode (Blind Hub)
Edge nodes encrypt all transcript content and event payloads before sending to the hub. The hub stores opaque blobs it cannot decrypt. Dashboard users must have the edge's decryption key (distributed via QR or secure channel) to view content.

The hub still provides aggregation (session counts, states, health scores) from unencrypted metadata, but cannot read what agents are doing.

**Why it's OOD:** Happy Coder does E2E encryption, but with a centralized server model. Forgemux's edge/hub topology makes federated privacy natural -- the edge is the trust boundary, and the hub is blind by design.

### 6.7 Session Recipes and Outcome Marketplace
When a session successfully completes a verified contract (idea 6.1), store the recipe: initial prompt, template used, contract definition, and outcome summary. Build a searchable library of successful recipes.

Phase 1: Internal recipe library per team. "Last time we added auth middleware, here's the prompt and template that worked."
Phase 2: Shareable recipes across organizations. A marketplace of reliable agent runbooks.

**Why it's OOD:** Competitors treat each session as a one-off. No one captures and reuses successful patterns systematically.

### 6.8 Differential Session Replay
Instead of replaying a full transcript, compute a diff-based replay that shows only the meaningful state changes: files created/modified, tests run, commands executed, decisions made. Skip the noise (agent thinking, repeated output, scroll-back).

Render as a timeline with expandable nodes:
```
[0:00] Session started (template: feature-work, model: opus)
[0:12] Created src/auth/middleware.ts (47 lines)
[0:15] Modified src/routes/api.ts (+8 lines at L45)
[0:18] Ran: npm test (14 passed, 0 failed)
[0:20] Ran: npm run lint (0 errors)
[0:22] Created commit: "Add auth middleware"
[0:23] Session completed (Verified)
```

**Why it's OOD:** Everyone stores raw transcripts. No one computes a meaningful diff-summary that's reviewable in 30 seconds.

### 6.9 Cost-Predictive Session Scheduling
Before starting a session, estimate its token cost based on historical patterns:
- Similar prompts in past sessions
- Repository size and complexity
- Template/contract requirements
- Model pricing

Show the estimate: "This session will likely cost $2-5 and take 15-30 minutes based on 12 similar past sessions." Allow cost caps that auto-terminate sessions exceeding budget.

At the fleet level: schedule lower-priority sessions during off-peak billing windows; batch similar work to amortize context loading costs.

**Why it's OOD:** No competitor predicts cost before starting. Everyone discovers cost after the fact.

### 6.10 Agent-Agnostic Event Protocol (Community Standard)
Define and publish a lightweight event protocol that any agent CLI can adopt:

```json
{"v":1, "event":"state", "state":"working", "ts":"..."}
{"v":1, "event":"tool", "tool":"bash", "cmd":"npm test", "ts":"..."}
{"v":1, "event":"input_needed", "prompt":"Allow file write?", "ts":"..."}
{"v":1, "event":"done", "outcome":"success", "ts":"..."}
```

Agent writes to `$FORGEMUX_EVENT_SOCKET` (Unix socket) or `$FORGEMUX_EVENT_FILE` (append-only file). Forgemux reads structured events instead of parsing PTY output. For agents that don't support the protocol, fall back to the current StateDetector.

Publish as an open spec. If adopted by even one agent CLI, Forgemux becomes the reference implementation and gains ecosystem gravity.

**Why it's OOD:** Everyone is building private adapters for each agent. No one is trying to define a cross-agent standard. The tool that defines the standard becomes the platform.

---

## 7. Competitor Capability Matrix

| Capability | Forgemux | Agent Deck | Superset | Happy | claudedash | Mission Control | Beehive |
|---|---|---|---|---|---|---|---|
| Session durability | tmux (best) | tmux | node-pty | daemon | observer | spawn-exit | direct PTY |
| State machine | 7-state FSM | polling | implicit | versioned | read-only | N/A | none |
| Multi-node | edge/hub | none | device routing | server relay | none | none | none |
| Supervisor agent | Foreman (spec) | Conductor (shipped) | none | none | none | Daemon scheduler | none |
| Worktree lifecycle | basic flag | full lifecycle | per-task | none | detection | none | clone-based |
| Session fork | none | Claude resume | none | none | none | none | comb copy |
| E2E encryption | stream-level | none | none | full E2E | none | none | none |
| Typed protocol | planned | none | tRPC | Zod wire | compatibility doc | Zod schemas | none |
| MCP integration | none | pool + skills | server + desktop | none | MCP server | none | none |
| Mobile/web control | dashboard (basic) | web + PWA | Electron + web | React Native | SSE dashboard | web UI | desktop only |
| Push notifications | planned | web push + Telegram | in-app | Expo push | none | none | none |
| Cost tracking | JSONL parsing | none | PostHog | usage events | billing windows | none | none |
| Container isolation | cgroups (planned) | Docker | none | none | none | none | none |
| Credential scrubbing | none | none | none | none | none | 14-pattern | none |
| Session search | none | fuzzy + regex | none | none | none | none | none |
| Quality gates | none | none | none | none | lint/test/type | none | none |
| Context health | none | none | none | none | 65%/75% warns | none | none |

---

## 8. Key Takeaways

1. **Forgemux has the best infrastructure foundation in the market.** The 7-state FSM with OCC, tmux-backed durability, and edge/hub topology are unmatched. No competitor would be easy to retrofit with these properties.

2. **The gap is above the infrastructure layer.** Ergonomics (templates, search, TUI), observability (context health, quality gates, insights), and integrations (webhooks, MCP, GitHub) are where competitors are pulling ahead.

3. **The Foreman is the highest-leverage feature.** Agent Deck's Conductor proves the concept works and is demanded. Forgemux's spec is more ambitious. Shipping it with lessons from Agent Deck's real-world iteration (context overflow, permission loops, crash recovery) would be a market-defining move.

4. **Security is an underinvested moat.** Most competitors ignore credential scrubbing, prompt fencing, and encryption. Forgemux can own the "enterprise-grade agent infrastructure" positioning by shipping Mission Control's security patterns on top of Happy's encryption model.

5. **The out-of-distribution opportunity is in verification and intelligence.** Session contracts, risk scoring, causality graphs, and cost prediction transform Forgemux from "a tool that runs agents" into "a system that understands what agents are doing and whether it's working." No competitor is building this layer.

6. **Define the protocol, become the platform.** An open agent event protocol is the single highest-leverage strategic move. If Forgemux defines how agents report their state, every tool that adopts the protocol becomes part of Forgemux's ecosystem -- even if they don't use Forgemux directly.

---

## 9. Exclusion Rationale

A reviewer flagged items from the source research that did not appear in the
main body. These are collected in two appendices: Appendix A (bullet-point
summary of omitted items, grouped by theme) and Appendix B (detailed
implementation patterns, code examples, integration blueprints, and an
additional set of OOD ideas with a priority matrix). This section explains the
editorial logic behind those omissions, covering both appendices.

### Why A.1 items (Reliability and State Fidelity) were excluded

**JSONL log watching, daemon lifecycle hardening, PID locks, orphan cleanup,
health checks.** These are *implementation techniques* rather than *market
themes or strategic positions*. The report intentionally operates at two
altitudes: market dimensions (section 3) and build recommendations (section 5).
Implementation details like PID lock files and orphan tmux reaping are important
engineering work, but they don't differentiate products in buyer perception or
market positioning. They belong in a technical design doc, not a competitive
analysis.

That said, the exclusion of **ephemeral vs durable event classification** was a
genuine oversight. Happy Coder's distinction between persistent events
(replayable, stored) and ephemeral events (presence, typing indicators --
broadcast but never persisted) is a design decision with outsized impact on
storage costs, replay performance, and reconnect UX. It is partially covered
under recommendation 5.1 item 9 ("Structured session protocol... durability
flags") but deserved more explicit treatment. It is an architectural choice that
compounds -- getting it wrong early means retrofitting every consumer of the
event stream later.

**Wire protocol versioning and CLI/daemon compatibility checks** are subsumed
by section 2.6 ("Typed Protocols for Survival") and recommendation 5.1 item 9.
The report treats protocol robustness as a theme; the specific mechanism
(version headers, 426 responses on mismatch) is implementation.

**Notification delivery reliability (retry/backoff, fallback channels, rate
limiting)** is operational plumbing. The report covers notifications as a market
dimension (section 2.5, recommendation 5.1 item 7) but does not specify
delivery mechanics. This is a fair exclusion for a market-level document, but an
unfair one for a build plan -- any real notification system needs retry and
rate limiting. The right home for this is the notification system's design spec,
not the competitive analysis.

### Why A.2 items (Agent Integration and Protocols) were excluded

**Agent adapter trait, MCP-first Codex adapter, permission mode resolution,
feature-flag rollout.** These are internal implementation patterns for a single
codebase. The report covers the *strategic* version of this idea in section 6.10
("Agent-Agnostic Event Protocol") -- defining a cross-agent standard rather
than building per-agent adapters. The adapter trait is the tactical
implementation of that strategy, but it's not a competitive differentiator.
Every multi-agent tool builds adapters; what matters strategically is whether
you're building private adapters or defining a public protocol.

The **MCP-first Codex adapter** specifically is too narrow for a market report.
It's a single-agent, single-protocol integration detail. Similarly, permission
mode resolution layering and feature flags are standard software engineering
practices, not competitive intelligence.

### Why A.3 items (Remote Access and Inspection) were excluded

**QR pairing** is mentioned in section 2.5 under Happy Coder ("QR pairing")
but not elevated to a recommendation because it's a Phase 3+ UX feature with
limited strategic impact. It's nice-to-have for onboarding, but it doesn't
change market positioning.

**RPC inspection surface** (file read/list/diff/log for dashboard and Foreman)
is a legitimate omission worth acknowledging. The ability to inspect a session's
working directory without attaching a terminal is genuinely useful for both
the dashboard and the Foreman. Happy's research doc makes a strong case for it.
The reason it was excluded: it falls between the "structured session protocol"
(recommendation 5.1 item 9) and the "MCP self-introspection" (recommendation
5.1 item 8) -- it's a read-only subset of what MCP would provide. But it could
be built sooner and independently of MCP. A fair criticism of the report is
that it jumps to the MCP abstraction without acknowledging the simpler
intermediate step.

**Artifact store, local control server, machine-scoped presence, session-vs-
machine separation** are all valid architectural patterns that were subsumed by
higher-level recommendations. The artifact store is useful once the Foreman and
dashboard need to surface structured outputs (diffs, reports), but it's not a
competitive differentiator. The local control server is covered implicitly by
the edge daemon's existing HTTP API. Machine-scoped presence is covered by the
edge/hub topology discussion in section 3.4. Session-vs-machine separation is
an internal modeling choice, not a market dimension.

### Why A.4 items (Product UX Patterns) were excluded

This is the largest exclusion set and the most intentional. The appendix lists
feature inventories per competitor: claudedash's plan protocol, Agent Deck's
MCP socket pooling, Superset's Electric SQL sync, Beehive's hidden-display
terminals, Mission Control's cron scheduler, etc.

The report deliberately **synthesized** these into cross-competitor themes
(section 2) and market dimensions (section 3) rather than listing them per-
product. A competitive analysis that enumerates every feature of every
competitor becomes a feature matrix, not a strategic document. The capability
matrix in section 7 serves the enumeration purpose in compressed form.

Specific items worth calling out:

- **MCP socket pooling** (Agent Deck): genuinely novel engineering. 85-90%
  memory reduction when running 10+ sessions. Excluded because Forgemux doesn't
  manage MCP processes today, so pooling is not yet relevant. But if Forgemux
  ever wraps agent MCP configuration, this pattern should be adopted wholesale.
- **Electric SQL real-time sync** (Superset): interesting technology choice but
  tightly coupled to Superset's Postgres + Electron architecture. Not portable
  to Forgemux's Rust + edge/hub model. The equivalent recommendation (SSE/WS
  streaming) is in section 5.1 item 5.
- **Hidden-display terminals** (Beehive): a frontend rendering pattern for
  keeping background terminal instances alive. Relevant only to the dashboard's
  Phase 3 implementation, not to competitive positioning.
- **Cron scheduler** (Mission Control): explicitly excluded in the "What Not
  to Build" section. Forgemux should consume schedules from external systems,
  not own a scheduler.
- **Port discovery** (Superset): niche feature for "agent started a dev
  server" workflows. Low strategic value.
- **Prompt-builder SOP** (Mission Control): the pattern of telling agents NOT
  to do bookkeeping (let the system handle it) is a good implementation detail
  that belongs in the Foreman's design spec, not the competitive analysis.

### Why A.5 items (Additional Roadmap Items) were excluded

**Session handoff (`fmux transfer`) and CLI diagnostics (`fmux doctor`)** are
operational features that don't rise to the level of market themes or strategic
recommendations. `fmux doctor` is a hygiene feature that every mature CLI
should have -- it's not a differentiator. Session handoff (transferring a
running session between edge nodes) is architecturally interesting but is a
Phase 3+ concern that depends on the reliable stream protocol being complete.

### Why Appendix B items were excluded

Appendix B (from `docs/research/appendix.md`) provides detailed implementation
patterns, code examples, and integration blueprints. The exclusion logic falls
into four categories:

**Items that overlap with the main report but at a different altitude.**
Appendix B sections I.5 (Cost-Aware Scheduling), I.6 (Session Marketplace),
I.7 (Causality Graph), I.8 (Risk Scoring), I.9 (Zero-Install Attach), and
I.10 (Federated Privacy) are implementation-level expansions of ideas already
in section 6 of the main report (6.9, 6.7, 6.3, 6.4, 6.5, 6.6 respectively).
The main report presents these as strategic concepts with "Why it's OOD"
framing; Appendix B provides code sketches and implementation detail. Both are
useful, but the main report is a strategy document and the code sketches belong
in a design spec. Including both would have doubled the length of section 6
without adding strategic clarity.

**Items that are genuinely new and were missed.** Several Appendix B items
introduce material not covered in the main report:

- **B.A: MCP-first Codex adapter** -- Happy Coder's pattern of running Codex
  through MCP rather than PTY scraping is a concrete, working example of the
  structured-events-over-PTY approach. The main report advocates for this at
  the protocol level (6.10) but doesn't cite this as prior art. This is a gap.
  The Happy research showed a production implementation of exactly what section
  6.10 proposes as novel, which weakens the "OOD" framing. The honest
  correction: the *protocol standardization* is OOD; the *technique* of
  structured events from a specific agent is not -- Happy already does it for
  Codex.

- **B.A.4: Sandbox wrapping for MCP** -- Combining OS-level sandboxing with
  MCP-spawned agents so permission prompts can be bypassed safely. This is a
  security pattern the main report doesn't address. The report discusses
  container isolation (section 3.1 capability matrix, section 5.1 item 1
  credential scrubbing) but doesn't connect sandboxing to MCP permission
  bypass. This is a gap worth noting: sandbox + MCP is a stronger security
  story than either alone.

- **B.F.2: Post-session bookkeeping protocol** -- Mission Control's pattern of
  running hook scripts on session completion (on_success, on_failure,
  on_timeout). The main report mentions outbound webhooks (5.1 item 7) but
  doesn't cover local post-session hooks, which are simpler to implement and
  useful earlier in the roadmap. This was under-weighted.

- **B.F.3: Decision queue / human-in-the-loop protocol** -- Mission Control's
  structured decisions model (agent posts question, execution blocks, human
  answers). The main report covers WaitingInput detection and the "Needs
  attention" dashboard view (5.1 item 5) but doesn't propose surfacing agent
  questions as a structured queue with options, separate from the raw terminal.
  This is a meaningful UX improvement over "the terminal is waiting" and
  deserved explicit treatment.

- **B.I.2: Foreman as policy engine** -- Automated intervention via declarative
  policies (stall-timeout -> restart, cost-cap -> pause). The main report
  discusses the Foreman extensively (section 2.3, takeaway 3) but frames it as
  a meta-agent, not as a rule-based policy engine. The policy engine framing is
  complementary: rules handle predictable conditions (cost cap, stall timeout),
  the meta-agent handles ambiguous ones (code quality judgment, prioritization).
  The main report should have presented both modes.

- **B.I.3: Session replays with branching** -- Record session as event stream,
  replay in browser with step-through, fork replay at any point. This extends
  section 6.8 (Differential Session Replay) with the branching concept -- not
  just viewing a replay but forking from any point to create a new session.
  This is a genuinely distinct idea that the main report's version doesn't
  capture.

- **B.I.4: Multi-modal session attach** -- Terminal, file tree, diff view, and
  structured log view as four separate attach modes. The main report assumes
  terminal attach as the primary interaction and read-only terminal view for
  zero-install sharing (6.5). Multi-modal attach is a richer concept: not
  every interaction needs a terminal. The diff view and file tree view are
  higher-value for reviewers than a raw terminal stream.

**Items that are integration blueprints, not competitive intelligence.**
Appendix B section J (Integration Opportunities) describes bidirectional
integration patterns with each competitor (Mission Control, Agent Deck,
Superset, Happy Coder). These are partnership/ecosystem strategies, not
competitive analysis. The main report takes a competitive lens ("where are we
stronger, where are we weaker, what should we build"). Integration blueprints
are valuable for product planning but are a different document type. They were
excluded to maintain the report's analytical focus.

**Items that are effort estimates.** Appendix B section K (Priority Matrix)
includes time estimates for each item. The main report deliberately avoids
effort estimates because they're unreliable without engineering review and can
distort prioritization (low-effort items get built regardless of strategic
value). The priority ordering in section 5.1 is based on strategic leverage,
not implementation cost. That said, the effort column in Appendix B's matrix
is useful as a *complement* to the strategic ordering -- it helps an
engineering team sequence work within a priority tier.

### Summary of exclusion principles

The report applied three filters when deciding what to include:

1. **Market-level vs implementation-level.** If an item is a competitive
   dimension that buyers perceive or that shifts market positioning, it belongs
   in the report. If it's an engineering technique for building one of those
   dimensions, it belongs in a design spec.

2. **Cross-competitor pattern vs single-product feature.** If 3+ competitors
   exhibit the same pattern, it's a market theme. If it's unique to one
   competitor, it's a feature inventory item (captured in the capability matrix,
   not the narrative).

3. **Strategic leverage vs operational hygiene.** Features that change
   Forgemux's competitive position (session contracts, agent protocol, attention
   budgeting) get narrative treatment. Features that improve reliability without
   changing positioning (PID locks, retry backoff, health checks) are important
   but don't belong in a strategy document.

The honest gaps in this filtering:

- **Ephemeral vs durable event classification** and the **RPC inspection
  surface** (Appendix A) were under-weighted. Both are architectural decisions
  with strategic implications that deserved more than a passing mention in a
  recommendation bullet point.
- **MCP-first Codex adapter** (Appendix B.A) is prior art that weakens the
  "OOD" framing of section 6.10. The protocol standardization is novel; the
  underlying technique is not.
- **Decision queue** (Appendix B.F.3) and **post-session hooks** (Appendix
  B.F.2) are practical features that bridge infrastructure and UX in ways the
  main report's webhook recommendation doesn't fully capture.
- **Foreman as policy engine** (Appendix B.I.2) and **multi-modal attach**
  (Appendix B.I.4) are genuinely distinct concepts that the main report's
  Foreman and attach discussions should have included.
- **Session replay with branching** (Appendix B.I.3) extends the main report's
  differential replay idea in a direction worth calling out explicitly.

---

## Appendix A. Notable Items From Source Docs (Not Reflected Above)

This appendix captures additional findings from the research set that are not
explicitly covered in the main body above, grouped by theme.

### A.1 Reliability and State Fidelity
- Agent JSONL log watching for state detection (Claude/Codex), plus structured event logs alongside raw transcripts.
- Daemon lifecycle hardening: PID lock, daemon state persistence, version negotiation, orphan tmux cleanup, health checks.
- Wire protocol versioning + CLI/daemon compatibility checks.
- Ephemeral vs durable event classification with ring-buffer replay on attach.
- Notification delivery reliability: retry/backoff, fallback channels, delivery logs, rate limiting.

### A.2 Agent Integration and Protocols
- Agent adapter trait to consolidate per-agent logic.
- MCP-first Codex adapter with structured events, turn lifecycle tracking, tool-call mapping, and reasoning channel separation.
- Permission mode resolution layering (defaults → session override → sandbox override).
- Feature-flag rollout for new session protocol formats.

### A.3 Remote Access and Inspection
- QR pairing flow for browser/mobile auth.
- RPC inspection surface (file read/list/diff/log) for dashboard and Foreman.
- Artifact store for session attachments (diffs, reports, snapshots).
- Local control server for daemon IPC and a machine-scoped presence channel.
- Session-vs-machine separation in sync model; usage-report events as first-class telemetry.

### A.4 Product UX Patterns
- Claudedash plan protocol (`queue.md` + `execution.log`) and compatibility contracts for watched files.
- Single-connection SSE + caching patterns for dashboard data layer.
- Worktree observability details (branch, dirty, ahead/behind) as a first-class view.
- Agent Deck: MCP socket pooling, Docker sandbox mode, web/PWA + web push, profile/grouping, parent/child sessions, tmux status notifications, built-in search, skill docs/llms.txt.
- Superset: Electric SQL real-time sync, device presence + command routing, wrapper scripts + env injection, workspace presets, port discovery/service links, built-in diff viewer, OAuth/RBAC, billing, analytics.
- Beehive: direct-PTY architecture, hidden-display terminals, layout persistence, preflight/onboarding flow, Hive/Comb workspace model, desktop packaging + auto-updater.
- Mission Control: cron scheduler, retry queue, health monitor + PID tracking, prompt-builder SOP to prevent agents writing to task state, Zod-validated JSON data layer.

### A.5 Additional Roadmap Items
- Session handoff workflow (`fmux transfer`).
- CLI diagnostics command (`fmux doctor`).

---

## Appendix B. Detailed Implementation Patterns and Integration Blueprints

Source: `docs/research/appendix.md`

This appendix provides code-level detail, architectural patterns, and
integration blueprints that supplement the strategic analysis in the main body.
See section 9 ("Why Appendix B items were excluded") for editorial rationale.

### B.A MCP-First Codex Adapter

**B.A.1 Structured Events vs PTY.**
Happy Coder runs Codex through an MCP-based adapter (`codex mcp-server`) and
consumes structured JSON-RPC events instead of scraping PTY output:

- Spawn `codex mcp-server` via MCP SDK alongside tmux session
- Structured events include: tool calls, diff events, reasoning deltas
- Use MCP events for telemetry while preserving PTY as human-visible interface

Benefits: higher-fidelity state detection without regex fragility, access to
reasoning/thinking channels, tool-call lifecycle events for audit trails.

**B.A.2 Unified Session Protocol.**
Happy converts Codex MCP messages into a compact, flat event stream with
explicit turn boundaries:

```json
{"v":1, "event":"turn-start", "turn":3, "ts":"2026-02-27T10:00:00Z"}
{"v":1, "event":"text", "content":"Analyzing...", "ts":"..."}
{"v":1, "event":"tool-call-start", "tool":"bash", "cmd":"npm test", "ts":"..."}
{"v":1, "event":"tool-call-end", "tool":"bash", "exit_code":0, "ts":"..."}
{"v":1, "event":"turn-end", "turn":3, "ts":"..."}
```

Benefits: agent-agnostic replay semantics, removes per-agent branching in UI,
enables ring-buffer replay without agent-specific logic.

**B.A.3 Turn Tracking.**
Explicit task lifecycle events (`task_started`, `task_complete`,
`turn_aborted`) enable per-turn metrics: cost, duration, tool usage. Real turn
boundaries vs idle-timeout inference.

**B.A.4 Sandbox Wrapping for MCP.**
Happy integrates `@anthropic-ai/sandbox-runtime` and wraps `codex mcp-server`
invocation with sandbox wrapper. When sandbox is enabled, Codex permission
prompts are bypassed (approval-policy: never), relying on OS-level
enforcement. Shows concrete path to apply OS-level sandboxing even when
process is spawned indirectly via MCP.

### B.B Local Control Server Architecture

**B.B.1 HTTP Control API.**
Happy's daemon exposes localhost HTTP server for daemon IPC:

```
GET  /list              -> List active sessions
POST /spawn             -> Start a session
POST /stop-session      -> Stop specific session
POST /stop              -> Shutdown daemon
```

Discovery: CLI reads `daemon.state.json` with port, PID, version, startedAt.

**B.B.2 Machine-Scoped Presence.**
Separate Socket.IO channel for machine-level heartbeats:

```json
{
  "machine_id": "edge-01",
  "pid": 1234,
  "version": "0.5.0",
  "sessions": 5,
  "uptime_seconds": 3600,
  "heartbeat_at": "2026-02-27T10:00:00Z"
}
```

**B.B.3 RPC Inspection Surface.**
Happy registers remote-callable tools over WebSocket: `bash.execute`,
`file.read`, `file.write`, `ripgrep.search`, `difftastic.diff`. Use cases:
dashboard inspect panel without full terminal attach, Foreman queries file
state directly, troubleshooting without disrupting session.

### B.C Implementation Patterns (Beehive Deep Dive)

**B.C.1 Hidden-Display Pattern.**
Render all open sessions simultaneously, hide inactive ones with CSS
`display: none`. Terminal state (scrollback, cursor) survives tab switches,
no reconnection lag. Memory bounded by open session count.

**B.C.2 Debounced Persistence.**
Debounce high-frequency writes (500ms) to avoid I/O storms. Hot-path state
(activity timestamps) uses debounced writes; state transitions use immediate
writes. Reduces disk I/O from O(sessions * poll_rate) to
O(sessions * flush_rate).

**B.C.3 Attention Tracking.**
Track `attached_clients` and `last_viewed_at` per session. Benefits:
notification suppression for active sessions, Foreman prioritizes unattended
sessions, dashboard sorts by recency.

**B.C.4 Preflight Guided Setup.**
Onboarding flow: check git, gh, gh auth status -> setup screen -> directory
picker with autocomplete -> config saved. Key: fail fast with remediation
instructions, autocomplete for paths, minimal required config.

**B.C.5 Session Fork with Worktree.**
`fmux fork S-a1b2c3d4 --branch experiment/v2`: create new worktree from
source's current commit, copy uncommitted changes via `git stash` + apply,
link sessions in metadata (parent_session field).

### B.D Advanced claudedash Features

**B.D.1 Context Snapshot and Rollback.**
Structured snapshots keyed by git commit hash. Recovery via
`claudedash recover <hash>`. Forgemux equivalent:
`fmux snapshot S-xxxx` / `fmux recover S-xxxx`.

**B.D.2 Prompt History Index.**
Serve prompt history from `history.jsonl`, cached by mtime. Enables: search
across sessions by prompt content, identify instructions that led to
stalls/failures, build session replay from initial prompt.

**B.D.3 Billing Window Awareness.**
Track 5-hour billing windows from Claude's `stats-cache.json`. Answer: how
much budget is left, should we throttle session creation, which sessions are
disproportionate consumers.

**B.D.4 Agent Heartbeat Registry.**
Agent registers with heartbeat every 60s; timeout marks agent dead.
Distinguishes "tmux alive" from "agent alive" to eliminate zombie session
blind spots.

**B.D.5 Insights Engine.**
Compute from execution logs: timeline (status transitions), velocity (tasks
per window), bottlenecks (tasks blocking others), success rate (DONE vs
FAILED). Forgemux adaptation: session velocity, stall frequency, cost
efficiency, agent comparison.

### B.E Database and Storage Patterns

**B.E.1 SQLite for Session Storage.**
Agent Deck uses SQLite with transactions and concurrent access. Benefits over
JSON files: atomic multi-record updates, query capability (find by status,
agent, repo), better concurrent access, migration support.

**B.E.2 Structured Logging with Ring Buffer.**
JSONL logs with ring buffer pattern. SIGUSR1 dumps to file for post-mortem.
Application: add `tracing` subscriber that maintains ring buffer for edge
daemon debugging.

**B.E.3 Primary Election.**
SQLite-based primary election to prevent multiple instances managing the same
sessions.

### B.F Context Management

**B.F.1 Context Compression.**
Mission Control generates `ai-context.md` -- ~650-token snapshot of workspace
state (vs ~10,000+ for raw JSON). Use cases: Foreman summarization, dashboard
lightweight endpoints, agent startup context.

**B.F.2 Post-Session Bookkeeping Protocol.**
Agents are explicitly told NOT to update task status; system handles
bookkeeping via hooks:

```toml
[post_session]
on_success = "scripts/post-success.sh"  # Called with session ID, transcript path
on_failure = "scripts/post-failure.sh"
on_timeout = "scripts/post-timeout.sh"
```

Benefits: parse output for structured report, POST to inbox API, update
WorkItem outcome, trigger retry.

**B.F.3 Decision Queue / Human-in-the-Loop.**
Agent posts question with options, execution blocks until answered. Structured
`Decision` record with session_id, question, options, context, answer.
Surfaces in dashboard as a queue, not just a raw terminal view.

### B.G Sync and Real-time Patterns

**B.G.1 Electric SQL-Style Real-Time Sync.**
Stream Postgres WAL changes to browser clients over HTTP/2. Forgemux
translation: SSE endpoint on forgehub pushing session record changes,
replacing dashboard polling.

**B.G.2 tRPC-Style Typed Contracts.**
Use `ts-rs` or `specta` to derive TypeScript interfaces from Rust structs.
Auto-generate `types.ts` during build. Prevents dashboard from drifting out
of sync with API.

**B.G.3 Agent Protocol Abstraction.**
Agent writes structured JSON events to `$FORGEMUX_EVENT_SOCKET` (Unix socket).
forged reads alongside PTY output. Fallback to StateDetector for non-compliant
agents.

### B.H State Management

**B.H.1 Agent Log File Watching.**
Parse Claude Code's JSONL session logs (`~/.claude/projects/<id>/session.jsonl`)
via inotify/kqueue. Detect WaitingInput from structured events vs regex,
detect Errored from error events vs exit codes, feed rich data to Foreman.

**B.H.2 Session Handoff Workflow.**
`fmux transfer <session-id> <user>`: new owner receives notification, previous
owner retains read-only access, transfer logged as lifecycle event, optional
context message. Use case: engineer goes off-shift, transfers live session
with full context.

**B.H.3 Version Compatibility Matrix.**
Exchange `VersionInfo` at connection time (binary, version, min_compatible,
protocol_version). Error on mismatch with actionable message.

### B.I Out-of-Distribution Ideas

**B.I.1 Session-Level Resource Governance.**
cgroups v2 for CPU/memory/PID limits, network namespace isolation, disk I/O
throttling, budget enforcement (max tokens per session). Advantage over Docker:
lighter, more suitable for CI/CD use cases.

**B.I.2 Foreman as Policy Engine.**
Automated intervention based on declarative policies:

```yaml
policies:
  - name: "stall-timeout"
    condition: "progress_stalled > 30min"
    action: "restart_and_retry"
  - name: "cost-cap"
    condition: "cost > $10 AND not_complete"
    action: "pause_and_notify"
```

Distinction from main report: policies handle predictable conditions
(rule-based), the Foreman meta-agent handles ambiguous ones (judgment-based).

**B.I.3 Session Replays with Branching.**
Record full session as event stream, replay in browser with step-through,
fork replay at any point to create new session, share replay URLs (like Loom
for coding sessions). Extends main report's differential replay (6.8) with
the branching concept.

**B.I.4 Multi-Modal Session Attach.**
Four attach modes: terminal (current), file tree view (read-only inspection),
diff view (current worktree changes), log view (structured events, not raw
PTY). Not every interaction needs a terminal.

**B.I.5 Cost-Aware Scheduling.**
Priority queue based on session value from WorkItem. Throttle low-priority
when billing window nearly exhausted, auto-pause sessions exceeding budget,
schedule low-priority during off-peak windows.

**B.I.6 Session Marketplace.**
Package session types as templates: `fmux template publish`, `fmux template
search`, `fmux template use`. Phases: internal recipe library, cross-org
sharing, public marketplace.

**B.I.7-10: Causality Graph, Risk Scoring, Zero-Install Attach, Federated
Privacy.**
These items duplicate main report sections 6.3, 6.4, 6.5, and 6.6
respectively, with implementation-level code sketches. See the main report
for strategic framing.

### B.J Integration Opportunities

**B.J.1 Mission Control.**
Bidirectional: FM -> MC (session completes -> POST task done; session errors ->
POST inbox report; session needs input -> POST decision). MC -> FM (task
assigned -> POST work-item; decision answered -> POST session input). Value:
MC gets durable execution, FM gets task demand. Neither becomes dependent.

**B.J.2 Agent Deck.**
AD sessions backed by Forgemux for durability. FM gets TUI UX patterns.

**B.J.3 Superset.**
Superset workspaces launched via Forgemux. FM gets polished product UX.

**B.J.4 Happy Coder.**
Adopt HC's E2E encryption model. HC uses Forgemux for session management.

### B.K Priority Matrix (Quick Reference)

| Item | Phase | Effort | Impact |
|------|-------|--------|--------|
| Agent log file watching | 0 | +1 week | High |
| Daemon lifecycle hardening | 0 | +2-3 days | Medium |
| Agent adapter trait | 0 | +2-3 days | Medium |
| CLI diagnostics (`fmux doctor`) | 0 | +1-2 days | Low |
| Wire protocol versioning | 2 | Minimal | High |
| CLI-daemon version check | 2 | +1-2 days | Medium |
| Ephemeral/persistent events | 2-3 | Minimal | High |
| QR pairing for browser | 3 | +3-4 days | Medium |
| Optimistic concurrency | 3 | +1-2 days | Low |
| E2E encryption | 3 | +1-2 weeks | Medium |
| Artifact store | 4-5 | +2-3 days | Low |
| RPC inspection surface | 4-5 | +3-4 days | Medium |
| Session handoff | 7 | +3-4 days | Low |

Total additional effort: ~5-7 weeks spread across existing roadmap phases.
