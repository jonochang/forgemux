# Forgemux — Test Plan

**Date:** February 2026
**Status:** Draft

---

## Principles

- **Test decisions, not plumbing.** Don't test clap parsing, serde derives, or trait impls that are exercised by higher-level tests.
- **One test per decision boundary.** Merge cases that exercise the same code path. Prefer a single test with all 7 states over 7 individual tests.
- **E2E over unit tests for thin wrappers.** The binaries are thin wrappers — test them via E2E, not by duplicating library tests.
- **No tests for unimplemented code.** Future phases get acceptance criteria to be expanded when the feature is built.
- **Every test must justify its maintenance cost.** If a test would break on a cosmetic change (CSS color, output wording) without catching a real bug, cut it.
- **Trace to scenarios.** Every E2E test maps to a scenario in `docs/specs/scenarios.md`. Scenario coverage is tracked in the traceability matrix.

---

## Conventions

- Unit tests in `#[cfg(test)] mod tests` within each source file
- Integration tests in `crates/<crate>/tests/` (require tmux, gated with `#[ignore]`)
- E2E tests in `tests/` at workspace root
- `FakeRunner` trait (already in `forged/src/lib.rs`) for all tmux/git shelling
- `tempfile::tempdir()` for filesystem isolation
- Run via `nix develop --command cargo test --workspace`

---

## 1. forgemux-core

### SessionId

| ID | Test | Expected |
|----|------|----------|
| CORE-01 | `SessionId::new()` format and length | Matches `^S-[0-9a-f]{4}$`, length 6 |

**Rationale:** Format is a contract used by tmux session naming and persistence paths. Uniqueness is guaranteed by UUID — not worth testing probabilistically. Display/AsRef covered by usage in store and CLI tests.

### SessionStore

| ID | Test | Expected |
|----|------|----------|
| CORE-02 | Save, load, update roundtrip | Save record, load matches. Modify state, save again, load returns updated state. Verifies `ensure_dirs` creates directory. |
| CORE-03 | Load missing ID → `SessionNotFound` | Error variant with ID in message |
| CORE-04 | List returns all saved sessions | Save 3 sessions, list returns 3. Empty dir returns empty vec. |
| CORE-05 | Corrupted JSON file → serde error on load | Write garbage to session file, load returns `CoreError::Serde` |

**Rationale:** Roundtrip (CORE-02) is the most important — it exercises save, load, ensure_dirs, and overwrite in one test. Missing ID and corruption are the two real failure modes.

### RepoRoot

| ID | Test | Expected |
|----|------|----------|
| CORE-06 | Discovers git root from nested subdirectory | `git init repo`, discover from `repo/a/b` returns `repo` |
| CORE-07 | Returns `None` for non-git directory | Temp dir with no `.git` anywhere up the tree |
| CORE-08 | Works with git worktrees (`.git` file) | `git worktree add`, discover from worktree returns worktree root |

**Rationale:** Nested discovery is the primary use case. Non-git fallback and worktree support are the two edge cases that matter.

### StateDetector

This is the critical differentiator per the spec and scenarios (D, E5: state derived from tmux existence + sidecar heartbeat + PTY activity). Tests focus on **decision boundaries**.

| ID | Test | Expected |
|----|------|----------|
| CORE-09 | Process dead: exit 0 → Terminated, exit nonzero → Errored, no exit code → Errored | Three assertions in one test |
| CORE-10 | Process alive + recent I/O → Running | I/O within both thresholds |
| CORE-11 | Process alive + idle past threshold + no prompt → Idle | Past idle_threshold, no prompt match |
| CORE-12 | Process alive + prompt match + past waiting threshold → WaitingInput | Both conditions met |
| CORE-13 | Prompt match but within waiting threshold → Running (not WaitingInput) | Key boundary: timing matters |
| CORE-14 | No prompt match + past waiting threshold → Idle (not WaitingInput) | Key boundary: prompt required |
| CORE-15 | Claude `(?m)^>\s*$` matches `">"` but not `"> some text"` | True positive and false negative in one test |

**Rationale:** CORE-13 and CORE-14 are the most valuable — they verify the two conditions (prompt AND time) are conjunctive, not disjunctive. CORE-15 catches prompt pattern regressions.

### Session Sorting

| ID | Test | Expected |
|----|------|----------|
| CORE-16 | Full priority order with tiebreaker | Create one session per state (all 7). Verify sort order: WaitingInput < Running < Idle < Errored < Terminated < Provisioning < Starting. Two sessions with same state sort by `last_activity_at` descending. |

**Rationale:** One test with all states. Exercises the sort used by `fmux ls` and the hub `/sessions` endpoint (Scenarios C, D).

### SessionManager

| ID | Test | Expected |
|----|------|----------|
| CORE-17 | `create_session` discovers git root and persists | Create inside nested git dir. `repo_root` is git root. Record loadable from store. |
| CORE-18 | `create_session` non-git dir uses canonical path | Create in plain dir. `repo_root` is canonicalized. |
| CORE-19 | `create_session_with_role` sets Foreman role | Role is `Foreman { watch_scope, intervention }` after creation |

---

## 2. forged — Edge Daemon Library

### Session Lifecycle

| ID | Test | Expected |
|----|------|----------|
| FGED-01 | `start_session` happy path | FakeRunner captures: `tmux new-session -d -s <id> -- claude`, then `pipe-pane`. Final persisted state is `Running`. |
| FGED-02 | `start_session` tmux failure | FakeRunner fails. Result is `Err`. Persisted state is `Errored`. |
| FGED-03 | `stop_session` terminates and persists | FakeRunner captures `kill-session -t <id>`. Persisted state is `Terminated`. |
| FGED-04 | `stop_session` for missing session → error | Session ID not in store. Result is `Err`. |
| FGED-05 | `start_session` with Codex agent | FakeRunner captures `-- codex` in tmux args. Verifies both agent types work through the same lifecycle path. |

**Rationale:** FGED-05 added per Scenario B — agent type must not affect lifecycle behavior. Happy path, failure, and stop cover the core contract.

### Worktree Support

| ID | Test | Expected |
|----|------|----------|
| FGED-06 | Start with worktree creates worktree and persists metadata | FakeRunner captures `git worktree add -b <branch> <path>`. Default path is `<repo>/.forgemux/worktrees/<branch>`. Metadata JSON written to `data_dir/worktrees/<id>.json`. |
| FGED-07 | Worktree fails if path exists | Pre-create the path. Returns error containing "already exists". |

**Rationale:** FGED-06 combines default path logic, git invocation, and metadata persistence. FGED-07 is the one guard clause. Worktree creation is central to the session start flow (Scenario A1).

### State Refresh

| ID | Test | Expected |
|----|------|----------|
| FGED-08 | `refresh_states` detects state changes and notifies | Create two sessions. FakeRunner returns capture-pane output with prompt pattern for one. After refresh: one session updated to WaitingInput, notification engine fires for the changed session only. |

**Rationale:** One integration-style test that exercises the full refresh pipeline (capture → detect → update → notify → sort). This is the mechanism behind state displayed in Scenarios C and D.

### Transcript Logs

| ID | Test | Expected |
|----|------|----------|
| FGED-09 | `logs` tails transcript file; missing transcript returns empty | Write 100-line file. `logs(id, 10)` returns last 10 lines. Missing transcript returns empty string. |
| FGED-10 | Transcript survives session termination | Start session (FakeRunner), stop session. Transcript file at `data_dir/transcripts/<id>.log` still exists and is readable. |

**Rationale:** FGED-10 added per Scenario H1 — transcripts must be available after termination for audit review. The spec explicitly states "full transcript and lifecycle event log are retained."

### Notifications

| ID | Test | Expected |
|----|------|----------|
| FGED-11 | Debounce suppresses duplicate, allows after expiry | Fire twice within debounce → 1 invocation. Fire again after debounce → 2 invocations. Different session or event → independent (both fire). |
| FGED-12 | Hook dispatch: desktop calls `notify-send`, command calls program with rendered template | Desktop hook → FakeRunner captures `notify-send`. Command hook → FakeRunner captures program with `{{session_id}}` → actual ID, `{{state}}` → human label, `{{agent}}` → "Claude"/"Codex". |
| FGED-13 | State routing: WaitingInput → on_waiting_input, Errored → on_error, Running → no hooks | Configure hooks for each event type. Verify correct routing. Running triggers nothing. |

---

## 3. forgehub — Hub Server Library

| ID | Test | Expected |
|----|------|----------|
| HUB-01 | `HubConfig::load` parses valid TOML, rejects invalid | Load fixture file → fields match. Load garbage → error. |
| HUB-02 | `list_sessions` aggregates across edges, sorted | Two edges with sessions (one WaitingInput, one Running). Result has both, WaitingInput first. |
| HUB-03 | `list_sessions` with unavailable edge → error with context | One edge has bad path. Error message contains edge ID. |

**Rationale:** Aggregation is the hub's core value (Scenario D: dashboard shows sessions across edge nodes).

---

## 4. Hub HTTP Server (integration test)

| ID | Test | Expected |
|----|------|----------|
| HTTP-01 | `/health` returns `{"status":"healthy"}` | 200, JSON body |
| HTTP-02 | `/sessions` returns sorted JSON array with session metadata | Populate edge data dirs. Response is JSON array with fields: `id`, `agent`, `model`, `state`, `repo_root`. WaitingInput first. Empty edges → `[]`. |
| HTTP-03 | `/ws` echo | Connect WebSocket, send "ping", receive "ping". Close frame is clean. |
| HTTP-04 | `/` serves dashboard HTML | Response contains `<title>Forgemux Dashboard</title>` |

**Rationale:** HTTP-02 updated to verify the response includes the metadata fields shown in Scenario D's dashboard table (id, agent, model, state, repo). One test per endpoint.

---

## 5. End-to-End (require tmux)

These are the highest-value tests. Each maps to one or more user scenarios from `docs/specs/scenarios.md`.

| ID | Scenario | Test | Steps | Expected |
|----|----------|------|-------|----------|
| E2E-01 | A (happy path) | Full session lifecycle: start, list, status, logs, stop | `fmux start` → `fmux ls` → `fmux status <id>` → `fmux logs <id>` → `fmux stop <id>` → `fmux ls` | Session ID printed on start. Session appears in ls with agent, model, state. Status shows detail. Transcript has content. Stop terminates. Final ls shows terminated. |
| E2E-02 | C (reattach) | Session survives detach; reattach to idle session | `fmux start` → `fmux detach <id>` → wait → `fmux ls` → `fmux attach <id>` | Session still listed after detach. Session reachable after reattach. |
| E2E-03 | A1/D/E5 | State detection: WaitingInput | Start session with mock agent that prints prompt pattern and blocks on stdin. Run `fmux watch` (one cycle). | Session shows `waiting` state. This is the core value proposition — "which agents need me right now?" |
| E2E-04 | G2 | State detection: Errored | Start session with command that exits nonzero. Wait. `fmux ls`. | Session shows `errored` state. |
| E2E-05 | A1 | Worktree creation and isolation | `fmux start --worktree --branch test-br` in a git repo. | Worktree at `.forgemux/worktrees/test-br` exists on disk. Session running inside worktree. Branch `test-br` created in git. |
| E2E-06 | B | Codex agent works identically to Claude | `fmux start --agent codex` → `fmux ls` → `fmux stop <id>` | Same lifecycle behavior as Claude. Agent type shown correctly in listing. |
| E2E-07 | D | Hub serves dashboard and sessions API | Start `forgehub run`. `curl /health`, `curl /sessions`. | Health returns JSON. Sessions returns array with session metadata fields. |
| E2E-08 | C | Multiple sessions sorted by state priority | Start 3 sessions. Arrange one WaitingInput, one Running, one Idle. `fmux ls`. | WaitingInput listed first, then Running, then Idle. |
| E2E-09 | H1 | Transcript retained after termination | `fmux start` → wait for output → `fmux stop <id>` → `fmux logs <id>` | Transcript still readable after session terminated. Content is non-empty. |
| E2E-10 | G1 | Stop flushes transcript and cleans up | `fmux start --worktree --branch cleanup-test` → `fmux stop <id>` | Transcript file exists with content. Session state is Terminated. |

**Rationale:** E2E-06 added for Scenario B (both agent types). E2E-09 added for Scenario H1 (transcript survives termination). E2E-10 added for Scenario G1 (stop flushes transcript). Each test maps to a specific scenario.

---

## 6. Security

| ID | Test | Expected |
|----|------|----------|
| SEC-01 | Session ID format prevents tmux command injection | `SessionId::new()` output contains only `S-` and hex chars. Construct a malicious ID like `S-; rm -rf /` and verify it cannot be produced by `SessionId::new()`. |
| SEC-02 | `pipe-pane` command construction is safe | Inspect the shell command string passed to `pipe-pane`. Session ID is used in a file path, not as a shell argument. |
| SEC-03 | Credentials never appear in session listing or API output | Start session with config containing `ANTHROPIC_API_KEY_FILE`. `fmux ls`, `fmux status`, and `/sessions` output contain no key file paths or key values. |

**Rationale:** Scenario preconditions state "credentials are present on the edge node and never leave it." SEC-03 verifies the API/CLI boundary. SEC-01/02 prevent injection via the tmux interface.

---

## 7. Performance

| ID | Test | Expected |
|----|------|----------|
| PERF-01 | Session store with 1000 sessions | Write 1000 session JSON files. `list()` returns all 1000. Measure time < 2s. |
| PERF-02 | `logs --tail 100` on large transcript | Write 100MB transcript file. `logs(id, 100)` returns 100 lines in < 500ms. |

**Rationale:** These catch the realistic scaling bottlenecks: directory-scan I/O (many sessions per Scenario D) and large-file tail (long-running sessions per Scenario H1).

---

## 8. Future Phases — Acceptance Criteria

These are **not test cases yet**. They are acceptance criteria to be expanded into tests when each phase is implemented. Scenario references indicate which user scenarios drive each criterion.

### Phase 1 — Notifications
- Desktop notification (`notify-send`/`osascript`) fires on `Running → WaitingInput`
- Webhook POST with template-rendered body fires on state transition
- Debounce window prevents duplicate notifications
- `fmux watch` refreshes display at configured interval

### Phase 2 — Hub Multi-Node
- `forged` registers with hub and maintains heartbeat *(Scenario A1: session registered in dashboard)*
- `fmux ls` without `--edge` returns sessions from all edges *(Scenario D: live overview across edge nodes)*
- `fmux edges` lists connected edge nodes with health
- Edge disconnect/reconnect detected by hub
- mTLS enforced on edge ↔ hub communication *(Preconditions: encrypted and authenticated)*

### Phase 3 — Browser Attach *(Scenarios A3, E3, F)*
- Browser connects via WebSocket, renders terminal via xterm.js *(Scenario A3: full PTY fidelity in browser)*
- SSH and browser attach coexist on same session simultaneously *(Scenario A3: tmux multiplexing)*
- Read-only browser attach prevents keyboard input *(Scenario F1: tech lead observes without interrupting)*
- Read-only SSH attach via `tmux attach -r` works *(Scenario F2)*
- Multiple read-only observers can attach simultaneously *(Scenario F3)*
- Ring buffer drops frames under backpressure (doesn't block agent)
- JWT/authentication required before browser attach is permitted *(Scenario A3: browser authenticates)*
- Browser disconnect does not terminate session

### Phase 4 — Dashboard *(Scenarios D, E)*
- Live session list updates via WebSocket push, no polling *(Scenario E5: live telemetry)*
- WaitingInput sessions visually highlighted *(Scenario D: state column)*
- Session card shows: edge, agent, model, state, worktree, tokens, cost, duration, CPU, memory, last active *(Scenario D table, E5 table)*
- Click **Attach (Web)** opens xterm.js terminal inline *(Scenario E3)*
- Click **Observe** opens read-only terminal *(Scenario F1)*
- **New Session** form submits `POST /api/sessions` with edge, agent, model, workspace *(Scenario E1/E2)*
- **Start & Attach** collapses session creation and attach into one action *(Scenario E: fewer-clicks variant)*
- **Copy SSH attach** button copies `ssh edge -t 'tmux attach -t <id>'` to clipboard *(Scenario E4)*
- Session detail view shows transcript *(Scenario H1)* and lifecycle events *(Scenario H2)*
- Lifecycle events include: SessionCreated, WorktreeCreated, StateChanged, ClientAttached/Detached, SessionTerminated, WorktreeRemoved *(Scenario H2 event log)*

### Phase 5 — Foreman
- Foreman session reads other sessions' transcripts and state
- Produces structured supervision reports
- Advisory mode: reports only, no cross-session interaction
- Assisted mode: proposes commands, engineer approves before injection
- Autonomous mode: injects commands and spawns helper sessions
- Foreman cannot escalate its own intervention level

---

## 9. Role + GitHub Handoffs (Epic)

These scenarios are implemented as Cucumber integration tests in `crates/forgehub/tests/features/role_github_handoffs.feature`.

| ID | Scenario | Test | Expected |
|----|----------|------|----------|
| HANDOFF-01 | I1 | Create handoff with valid GitHub issue | `POST /handoffs` succeeds; handoff appears in target queue as `queued` |
| HANDOFF-02 | I1 | Reject unknown GitHub issue | `POST /handoffs` returns bad request when issue is missing |
| HANDOFF-03 | I2 | Claim lock | First `POST /handoffs/:id/claim` succeeds, second returns conflict |
| HANDOFF-04 | I3 | Approve then promote | Reviewer completes with `approve`, then promote creates queued `sre` handoff |
| HANDOFF-05 | I3 | Request changes loopback | Reviewer completes with `request_changes`; queued implementer handoff is created |
| HANDOFF-06 | I4 | GitHub close webhook | `POST /github/webhook` marks linked handoff `needs_attention` |
| HANDOFF-07 | I5 | GitHub write-back | Claim/complete/promote actions produce issue comments |

### Phase 6 — Sandboxing
- cgroup v2 enforces CPU and memory limits per session *(Preconditions: policy enforcement)*
- Network namespace isolates agent from external network
- Filesystem bind mount restricts agent to repo directory
- Policy violations logged as events

### Phase 7 — Access Control
- API tokens created, hashed (argon2), revocable
- RBAC: viewer (read-only), operator (manage sessions), admin (full control)
- All API endpoints enforce RBAC; unauthorized → 403
- Dashboard requires authentication *(Scenario E preconditions: SSO)*
- Admin can terminate any session remotely *(Scenario G3)*

### Phase 8 — Token Tracking *(Scenario H3)*
- Claude JSONL collector parses usage from `~/.config/claude/projects/`
- Codex JSONL collector handles missing token fields gracefully
- `fmux usage` shows per-session and aggregate token/cost data
- Token stats prefer provider-native reporting; fall back to estimation *(Scenario D: token stats)*
- Cost attributed per session, aggregatable by engineer/project/edge *(Scenario H3)*

### Phase 9 — Operational Maturity
- `forged drain` stops new sessions, waits for existing, force-kills after timeout
- Config hot-reload on file change
- `/metrics` endpoint returns Prometheus-compatible format
- `forgehub export csv` produces valid output
- Worktree cleanup (`git worktree remove`) on session termination *(Scenario G1)*

---

## Scenario Traceability Matrix

Maps each scenario from `docs/specs/scenarios.md` to test coverage.

| Scenario | Description | Phase 0 Tests | Future Criteria |
|----------|-------------|---------------|-----------------|
| **A1** | Start session (CLI) → worktree + tmux + sidecar | E2E-01, E2E-05, FGED-01, FGED-06 | Phase 2 (hub registration) |
| **A2** | SSH attach, detach, agent continues | E2E-02 | — |
| **A3** | Web attach, simultaneous SSH+web | — | Phase 3 (browser attach, multiplexing) |
| **B** | Codex agent — identical lifecycle | E2E-06, FGED-05 | — |
| **C** | Reattach after disconnect; list shows metadata | E2E-02, E2E-08, CORE-16 | — |
| **D** | Dashboard live overview with telemetry | HTTP-02, E2E-07 | Phase 4 (live dashboard, telemetry fields) |
| **E1** | Start session from browser | — | Phase 4 (New Session form, POST /api/sessions) |
| **E2** | Backend start path (worktree + sidecar) | FGED-01, FGED-06 | Phase 2 (hub→edge forwarding) |
| **E3** | Browser attach via WebSocket | — | Phase 3 (xterm.js, WebSocket bridge) |
| **E4** | Copy SSH attach command | — | Phase 4 (Copy SSH attach button) |
| **E5** | Dashboard live state fields | HTTP-02 (partial) | Phase 4 (full telemetry: tokens, CPU, cost) |
| **F1** | Read-only browser observe | — | Phase 3 (read-only attach) |
| **F2** | Read-only SSH (`tmux attach -r`) | — | Phase 3 (read-only SSH) |
| **F3** | Multiple simultaneous observers | — | Phase 3 (multiplexing) |
| **G1** | Engineer terminates → cleanup | E2E-01, E2E-10, FGED-03 | Phase 9 (worktree removal) |
| **G2** | Idle timeout auto-terminates | E2E-04 (errored detection) | Phase 0 gap: idle timeout policy not yet tested E2E |
| **G3** | Admin remote kill via dashboard | — | Phase 7 (admin RBAC + terminate) |
| **H1** | View transcript after session ends | E2E-09, FGED-09, FGED-10 | Phase 4 (transcript viewer in dashboard) |
| **H2** | Lifecycle event log | — | Phase 4 (event log in session detail) |
| **H3** | Usage accounting (tokens, cost) | — | Phase 8 (token tracking) |

### Identified Gaps

| Gap | Scenario | Status | Action |
|-----|----------|--------|--------|
| Idle timeout auto-terminates session | G2 | Phase 0 spec, not yet E2E tested | Add E2E test when idle timeout policy is implemented in `forged` |
| Worktree cleanup on termination | G1 | Described in spec, not yet implemented | Add to FGED-03 when `git worktree remove` is added to stop path |
| Lifecycle event logging | H2 | Not yet implemented | Add unit test when event store is built (Phase 2+) |
| `POST /api/sessions` contract | E1/E2 | Not yet implemented | Add HTTP test when hub session creation API is built (Phase 2+) |

---

## Appendix: Existing Test Coverage

16 tests exist today. Mapping to this plan:

| Existing test | Plan ID |
|---|---|
| `session_id_has_prefix` | CORE-01 |
| `session_store_roundtrip` | CORE-02 |
| `repo_root_discovers_git_root` | CORE-06 |
| `session_manager_uses_worktree_root` | CORE-08 |
| `session_manager_falls_back_to_path_when_not_git` | CORE-18 |
| `state_detector_marks_waiting_input` | CORE-12 |
| `state_detector_marks_idle` | CORE-11 |
| `state_detector_marks_running` | CORE-10 |
| `state_detector_marks_errored` | CORE-09 |
| `sort_sessions_prioritizes_waiting_input` | CORE-16 (partial) |
| `start_session_invokes_tmux_new_session` | FGED-01 |
| `start_session_records_error_on_tmux_failure` | FGED-02 |
| `notification_engine_debounces` | FGED-11 |
| `render_template_expands_session_values` | FGED-12 |
| `create_worktree_runs_git` | FGED-06 |
| `hub_service_aggregates_sessions` | HUB-02 |

---

## Test Count Summary

| Section | Tests |
|---------|-------|
| forgemux-core | 19 |
| forged library | 13 |
| forgehub library | 3 |
| Hub HTTP server | 4 |
| End-to-end | 10 |
| Security | 3 |
| Performance | 2 |
| **Total** | **54** |
| Future phase acceptance criteria | ~35 (not yet tests) |
