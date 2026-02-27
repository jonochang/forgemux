# Research: Happy Coder (Codex Path)

Source: `/home/jonochang/lib/jc/happy`

Happy Coder runs Codex through an MCP-based adapter and emits a unified session
protocol for real-time mobile/web rendering. This note focuses specifically on
Codex-related design choices that could improve Forgemux.

---

## 1. MCP-First Codex Adapter (Structured Events vs PTY)

Happy does not treat Codex as a plain TTY stream. It spawns `codex mcp-server`
via the MCP SDK and consumes structured JSON-RPC events instead of scraping PTY
output.

**Relevance to Forgemux:** Forgemux currently relies on tmux + PTY output
parsing for state detection. MCP gives richer signal (tool calls, diff events,
reasoning deltas) without regex heuristics.

**Possible approach:**
- Add a Codex MCP adapter that runs alongside the tmux session and writes a
  structured event log.
- Feed those events into the existing state detector to drive `WaitingInput`,
  `Running`, and `Error` transitions with higher fidelity.
- Preserve PTY as the human-visible interface; use MCP only for telemetry.

---

## 2. Unified Session Protocol for Codex Streams

Happy converts Codex MCP messages into a compact, flat event stream with explicit
turn boundaries and tool-call lifecycle events. The same protocol is used by the
mobile/web clients across Claude and Codex.

**Relevance to Forgemux:** Forgemux will need a stable, typed event stream for
WebSocket attach and dashboard replay. A unified protocol removes per-agent
branching in the UI and allows ring-buffer replay to be agent-agnostic.

**Possible approach:**
- Define a `forgemux-session-protocol` envelope with `turn-start`, `turn-end`,
  `text`, `tool-call-start`, `tool-call-end`, `file`, and `service` events.
- Convert Codex MCP events into this protocol inside the edge daemon.
- Replay events to new WebSocket clients before live streaming.

---

## 3. Turn Tracking and Lifecycle Events

Happy tracks Codex task lifecycle explicitly (`task_started`, `task_complete`,
`turn_aborted`) and emits `turn-start`/`turn-end` events. UI treats these as
session boundaries for grouping.

**Relevance to Forgemux:** Forgemux currently infers turn boundaries from idle
timeouts and output markers. Codex task events would allow real turn-level
metrics: per-turn cost, duration, and tool usage.

**Possible approach:**
- Map MCP `task_*` events into explicit turn boundaries.
- Store per-turn metadata for dashboards and Foreman summaries.

---

## 4. Tool-Call Lifecycle Mapping (Commands + Patches)

Happy maps Codex MCP events to tool-call lifecycle entries:
- `exec_command_begin`/`exec_command_end` -> `tool-call-start`/`tool-call-end`
- `patch_apply_begin`/`patch_apply_end` -> `tool-call-start`/`tool-call-end`

**Relevance to Forgemux:** This yields a clean audit trail of commands and
patches the agent executed, without scraping terminal output.

**Possible approach:**
- Emit tool-call events from MCP messages into the structured event log.
- Use these for audit trails, cost attribution, and Foreman summaries.

---

## 5. Reasoning/Thinking Channels as First-Class Events

Happy splits Codex reasoning deltas into `thinking: true` text events, letting
clients hide or show them separately from visible output.

**Relevance to Forgemux:** Forgemux could hide verbose reasoning by default in
transcripts while preserving it for audit or debugging, reducing noise in the
dashboard.

**Possible approach:**
- Store reasoning events separately and mark them as non-default in UI.
- Offer a transcript filter toggle to include/exclude reasoning.

---

## 6. MCP-Compatible Sandbox Wrapping

Happy integrates `@anthropic-ai/sandbox-runtime` and wraps `codex mcp-server`
invocation with `sh -c "<sandbox-wrapped command>"` because the MCP SDK owns
the spawn call. When sandbox is enabled, Codex permission prompts are bypassed
(approval-policy: never), relying on OS-level enforcement.

**Relevance to Forgemux:** Forgemux already plans filesystem/network policies;
this shows a concrete path to apply OS-level sandboxing even when the process is
spawned indirectly via MCP.

**Possible approach:**
- Add an optional sandbox wrapper for Codex MCP sessions.
- When enabled, disable interactive permission prompts and trust the sandbox.
- Expose sandbox config in the CLI (workspace root, read/write allowlists,
  network allow/deny, localhost binding).

---

## 7. Permission Mode Resolution Layering

Happy centralizes permission mode selection (defaults, session overrides,
sandbox forcing) and maps app-level modes to provider-specific flags.

**Relevance to Forgemux:** Forgemux will likely need layered policies
(org-level, node-level, session-level). Codex needs explicit approval policy
flags when run headlessly.

**Possible approach:**
- Define a permission resolution pipeline: policy defaults -> session override
  -> sandbox override.
- Map final mode to Codex flags (approval policy + sandbox flags) and to Claude
  flags where relevant.

---

## 8. Compatibility Rollout via Feature Flags

Happy rolls out new Codex session protocol with a feature flag, while keeping
legacy formats supported on the client. This allowed gradual upgrades.

**Relevance to Forgemux:** Forgemux will need staged rollout of the WebSocket
protocol and dashboard. Feature flags lower the migration risk.

**Possible approach:**
- Gate the new event protocol behind an env flag in the edge daemon.
- Allow the dashboard to accept both legacy PTY logs and new structured events
  until migration completes.

---

## Summary

| Idea | Forgemux Gap | Priority |
| --- | --- | --- |
| MCP-based Codex adapter | PTY-only state detection | High |
| Unified session protocol | No typed event stream | High |
| Turn lifecycle tracking | No explicit turn boundaries | Medium |
| Tool-call lifecycle mapping | No structured command/patch audit | High |
| Reasoning channel separation | No filterable transcript layers | Medium |
| Sandbox wrapping for MCP | No OS-level sandbox for Codex | Medium |
| Permission resolution layering | Ad-hoc policy mapping | Medium |
| Feature-flag rollout | Risky protocol migrations | Low |
