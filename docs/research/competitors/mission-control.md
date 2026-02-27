# Mission Control Review (for Forgemux Ideas)

Date: February 27, 2026
Sources: /home/jonochang/lib/mission-control/README.md

## Snapshot
Mission Control is a local-first, agent-oriented task management system with a web UI and a daemon that can launch Claude Code sessions. It treats tasks, goals, agents, and messages as first-class data stored in JSON files. It positions itself as an "agent command center" for solo operators who want structure, delegations, and reporting.

Key points from the README:
- Product focus: task management for AI agents, not just humans.
- Data model: local JSON files for tasks, goals, projects, agents, skills, inbox, decisions, activity, daemon state.
- Execution: one-click launch of agent sessions via Claude Code; autonomous daemon can schedule and run tasks.
- UX: Eisenhower matrix, Kanban, goal hierarchy, inbox, decisions queue, global search.
- Agent features: predefined roles, custom agents, skills library, multi-agent tasks, orchestrator.
- API: token-optimized queries, pagination, and validation for write endpoints.
- Tests: significant validation and data-layer coverage.

## Comparison With Forgemux
Mission Control focuses on task and workflow orchestration at the human decision level. Forgemux focuses on durable, observable agent execution sessions at the infrastructure level. They overlap at the handoff point: a task becomes a session with lifecycle, outputs, and reporting.

Areas where Mission Control is ahead (relative to current Forgemux scope):
- Task and decision workflows that create the demand for sessions.
- Rich agent metadata (roles, instructions, skills) with reuse patterns.
- Built-in messaging and reporting channels (inbox, decisions queue).
- High-level orchestration commands (orchestrate, daily-plan, weekly-review).

Areas where Forgemux is stronger or more explicit:
- Edge/hub separation, multi-node execution, and durable session lifecycle.
- Session state machine and reliability model.
- Observability hooks and metrics-driven architecture.

## Ideas for Forgemux Inspired by Mission Control

**1) Task-to-Session Bridge**
- Introduce a minimal "task" layer at the hub that can map to one or more sessions. Keep it lightweight: a task record with intent, repo, agent spec, and desired outcome. The bridge can remain optional and configurable so Forgemux stays infrastructure-first.
- Provide an API endpoint to convert a task into a session launch with attached metadata and expected artifacts.

**2) Agent Registry and Skills Injection**
- Add a simple agent registry file in hub data storage that contains agent profiles, instructions, and default tools.
- Support an optional "skills" bundle that is injected into session prompts or provided as a sidecar context file. This aligns with the foreman model and could be a config-driven feature.

**3) Inbox and Decisions Queue**
- Add a lightweight inbox model at the hub for agent-to-human reports. Sessions can emit a report event that lands in the inbox.
- Add a decisions queue that is triggered by a session state of WaitingInput or a recognized "needs approval" event.

**4) Orchestration Commands and Schedules**
- Add hub-level commands similar to /orchestrate, /standup, /daily-plan, but scoped to sessions and pending work queues.
- Provide a cron-like scheduler in forgehub that can launch sessions or foreman checks on a cadence.

**5) UI Patterns for Operational Clarity**
- Borrow the "inbox + decisions" view to surface sessions that need human attention, not just raw state lists.
- Consider an "attention heatmap" view derived from WaitingInput and Idle states.
- Add quick filters and global search across sessions, transcripts, and reports.

**6) Activity Log and Audit**
- Store a structured activity log of session lifecycle events, user interventions, and foreman actions. This enables explainability and post-mortem workflows.

**7) Multi-Agent Task Cohesion**
- Allow a single "work item" to coordinate multiple sessions (lead + collaborators) with shared output expectations. This aligns with the foreman concept but provides more explicit grouping in the hub UI.

**8) Runbook-Style Session Templates**
- Provide templated session starters: repo context, setup steps, expected outputs, and reporting format. This can live as templates in hub config and be referenced by a session launch.

**9) Token-Optimized API and Sparse Fetch**
- Consider an API shape that supports sparse field selection for session lists, so UIs and CLIs can minimize bandwidth and context size.

**10) Validation and Local-First Storage Patterns**
- Mission Control's local JSON + Zod validation pattern can inform Forgemux configuration and data storage validation. For the hub, consider a strict schema for event records and transcripts, even if the backing store is SQLite or Postgres.

## Practical Integrations (Low Risk)
- Provide a compatibility mode where Forgemux can read a Mission Control task record and turn it into a session.
- Provide a CLI helper that accepts a "report" payload and emits it into the hub inbox model.
- Provide an optional "skills directory" that can be referenced by sessions without adding a new runtime dependency.

## Risks and Tensions
- Adding task management could dilute Forgemux's focus on infrastructure. Keep any task layer thin and optional.
- Skills injection and orchestration can overlap with Foreman responsibilities; keep the mental model clear: Foreman monitors execution, hub orchestrates scheduling.
- Local-first JSON storage is simple but can become a coordination bottleneck for multi-node deployments. If adopted, keep it as an edge-side artifact rather than hub-side.

## Suggested Next Steps
- Pick one low-risk integration: inbox events for session reports.
- Decide on a minimal agent registry schema, even if used only for prompt templates.
- Add a dashboard view focused on "needs attention" derived from WaitingInput and recent errors.
