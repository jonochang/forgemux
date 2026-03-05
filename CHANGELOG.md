# Changelog

## 0.1.24 - 2026-03-05

- Add chromiumoxide-driven dashboard BDD coverage for model selection.
- Pass selected model to Claude/Codex commands when starting sessions.
- Add stable test ids for attach/start UI controls.

## 0.1.23 - 2026-03-05

- Fix model dropdown selection syncing and session name payloads in the dashboard.
- Persist session name/goal when starting a session across the edge workflow.
- Add BDD session creation scenario covering name + model selection.

## 0.1.22 - 2026-03-05

- Add hub workspace seeding/config plus workspaces endpoints.
- Add workspace-scoped session listing and repo-root mapping.
- Add dashboard workspace switcher and workspace-scoped sessions/decisions.
- Add BDD workspace scenarios, including HTTP integration coverage.

## 0.1.21 - 2026-03-05

- Add configurable model probe args with timeouts to avoid hanging `forged check`.
- Stop running undocumented model probe commands by default.
- Update default Codex model list to `gpt-5.3-codex` and `gpt-5.2-codex`.

## 0.1.20 - 2026-03-05

- Add forged doctor checks for model probe commands (claude/codex/gemini/opencode).
- Auto-detect available models from installed agent tools and expose via edge config.

## 0.1.19 - 2026-03-04

- Add hub `/version` endpoint with discreet dashboard "about" link.
- Add model preset dropdown (with custom override) when creating sessions.

## 0.1.18 - 2026-03-03

- Add interactive `configure` flows for forgehub/forged plus `fmux configure`.
- Authenticate edge registration/heartbeat with hub tokens.
- Add replay File Tree tab and Atomic Merge placeholder CTA.
- Add Hub UX review notes.

## 0.1.17 - 2026-03-02
- Embed dashboard assets into the forgehub binary (no external dashboard files required).
- Session attach resumes from last seen event to avoid replay loops.

## 0.1.16 - 2026-03-02

- Hide stale terminal-state sessions (>3 days) from fleet dashboard.
- Add Replay and Attach action links to fleet session cards.
- Fix attach view terminal flickering on WebSocket reconnect.

## 0.1.15 - 2026-03-01

- Add Session tab creation form with forged instance selection and default repo loading.
- Wire replay diff proxy and add file tree/Atomic Merge placeholders.
- Fix risk threshold boundary and add property + frontend utility tests.
- Add hub tracing instrumentation and decision flow integration tests.

## 0.1.13 - 2026-02-27

- Add decision queue UI with repo filters and context panels.
- Wire decision queue to hub REST/WS streams and resolve actions.

## 0.1.12 - 2026-02-27

- Add Preact+HTM dashboard shell with fleet view.
- Serve legacy dashboard at `/legacy`.

## 0.1.11 - 2026-02-27

- Add decision flow endpoints on forged and forward to hub.
- Add credential scrubbing for decision contexts.
- Add tmux session environment variables for hooks.

## 0.1.10 - 2026-02-27

- Add session hub metadata types and risk scoring logic.
- Add hub session cache persistence and edge polling for session snapshots.

## 0.1.9 - 2026-02-27

- Add decision queue data model, hub CRUD, and websocket events.
- Add hub decision endpoints and default workspace seeding for decisions.
- Add decision types to forgemux-core.

## 0.1.8 - 2026-02-27

- Split forgemux-core into modules and add `Unreachable` session state.
- Introduce workspace/org core types plus goal/session ID fields on SessionRecord.
- Add forgehub SQLite migrations and DB bootstrap.

## 0.1.7 - 2026-02-27

- Add hub pairing flow and dashboard token-based auth.
- Add stream protocol version checks between fmux/forged/forgehub.
- Add optimistic concurrency to session records.
- Add optional stream encryption support.
- Add dashboard session detail view (logs/usage) and start-session form.

## 0.1.6 - 2026-02-27

- Add agent log watching to improve WaitingInput detection.
- Add forged PID lock and orphan tmux session cleanup.
- Add `fmux doctor` diagnostics for config and connectivity.
- Add notification retries, fallback behavior, and delivery logs.
- Update protocol specs with versioning and durable/ephemeral events.

## 0.1.5 - 2026-02-27

- Embed hub dashboard index.html in the binary for /.

- Fix hub config parsing to allow missing `edges`.
- Strip ANSI escape codes before prompt detection.
- Fix `fmux ls` tab-delimited output.

- Fetch remotes before worktree creation and track remote branches when present.

## 0.1.4 - 2026-02-26

- Added hub/edge token authentication and CLI token support.
- Added reliable stream protocol primitives, watch mode, and periodic snapshots.
- Added hub relay buffering for offline edges and dashboard offline input queueing.
- Added usage and metrics endpoints plus `fmux usage`.
- Added drain/export commands and rotate-cert stub.
- Added default repo support and data-dir based worktree layout.
- Improved NixOS install instructions and packaging (default.nix, git dependency).

## 0.1.0 - 2026-02-25

- Initial scaffold: CLI, edge daemon, hub, and dashboard placeholder.
