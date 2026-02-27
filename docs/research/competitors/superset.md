# Research: Superset Review and Forgemux Ideas

**Date:** February 27, 2026
**Sources reviewed:** `/home/jonochang/lib/superset/README.md`, `/home/jonochang/lib/superset/AGENTS.md`, `/home/jonochang/lib/superset/apps/desktop/docs/EXTERNAL_FILES.md`, `/home/jonochang/lib/superset/apps/marketing/docs/SEO_COMPARISON_PAGES_PLAN.md`

This document summarizes what Superset is building, compares it to forgemux's
current direction, and proposes concrete ideas for forgemux based on Superset's
product and infrastructure patterns.

---

## 1. Superset Snapshot

**Positioning:** "The terminal for coding agents." Superset is an
agent-agnostic orchestration layer that runs many CLI agents in parallel with
worktree isolation and built-in review/monitoring UX.

**Core features (from README):**
- Parallel agent execution (10+ agents at once).
- Git worktree isolation per task.
- Central monitoring with notifications.
- Built-in diff viewer / review UI.
- Agent-agnostic: runs Claude Code, Codex CLI, OpenCode, etc.

**Desktop app model:** Superset is a full desktop app (Electron), with local
terminal management, workspace management, and UI for sessions. The technical
docs emphasize terminal persistence, warm attach, and session lifecycle signals
to keep UI state correct when terminals are not mounted.

**Workspace isolation & hooks:** Superset writes wrapper scripts and hooks into
`~/.superset[-{workspace}]/`, and injects environment variables per terminal
session. This ensures:
- Per-workspace isolation (separate directories by workspace name).
- Agent wrapper binaries (`claude`, `codex`, `opencode`) that inject settings.
- Notification hook scripts and agent-specific integration (Claude settings JSON,
  OpenCode plugin).
- Shell integration that prepends the wrapper bin directory to PATH.

---

## 2. Comparison with Forgemux (Today)

**Forgemux today (from README):**
- Rust-based platform for durable, observable agent sessions.
- tmux-backed session layer (`forged`) with session capture.
- CLI (`fmux`) for start/attach/list, optional worktree start.
- Hub (`forgehub`) for aggregation and a lightweight dashboard placeholder.
- Prometheus metrics endpoints.

**Superset strengths vs forgemux:**
- Product polish around *agent orchestration UX* (diff review, notifications,
  workspace presets, hotkeys).
- First-class local worktree management with clear UX patterns.
- Deep integration with agent tooling via wrapper scripts + hooks.
- Explicit session lifecycle correctness in the UI even when terminals are not
  attached.

**Forgemux strengths vs Superset:**
- Clear separation of edge/hub architecture.
- tmux-backed durability and transcript capture.
- Infrastructure-first (metrics, daemon, optional hub).
- Rust service + CLI (lighter runtime, easier to run headless).

**Key gap:** Superset has more UX and agent-integration surface area; forgemux
has more infrastructure primitives but lacks the ergonomic workflows that drive
adoption in day-to-day use.

---

## 3. Ideas for Forgemux Inspired by Superset

1. **Per-workspace wrapper binaries + hooks**
   - Add `~/.forgemux[-{workspace}]/bin` with wrapper scripts for `claude`,
     `codex`, `opencode`, etc. These wrappers inject environment variables and
     hook config automatically, so agents emit lifecycle events to `forged`.
   - Benefit: integration without requiring users to configure each agent.

2. **Standardized session env vars**
   - Mirror Superset's approach by exporting `FORGEMUX_SESSION_ID`,
     `FORGEMUX_EDGE_ID`, `FORGEMUX_WORKTREE_PATH`, `FORGEMUX_ROOT_PATH`,
     `FORGEMUX_HOOK_VERSION`, etc.
   - Benefit: agents and scripts can report events or telemetry consistently.

3. **Notification hooks + state push**
   - Add a lightweight notification hook protocol so agents can signal
     `waiting-input`, `completed`, `errored` with minimal setup.
   - Benefit: improves state detection beyond PTY parsing and enables
     user-facing notifications.

4. **Worktree UX upgrades**
   - Superset distinguishes *worktree workspaces* from *branch workspaces*.
     Consider a similar distinction in `fmux` so users can create a task on the
     current branch without always requiring worktrees, while still offering
     worktree isolation as the default.

5. **Session management UI in dashboard**
   - A dedicated UI for session inventory: list sessions, last activity, kill
     idle sessions, and show resource use. Superset plans emphasize limiting
     runaway session counts and providing recovery tooling.

6. **Built-in diff review**
   - Superset's built-in diff viewer is a key differentiator. Forgemux could
     add a "review" mode in the hub dashboard: show diff for a session's
     worktree and allow applying/ignoring changes.
   - This can be incremental: start with a read-only diff from git status
     in the worktree.

7. **Workspace presets**
   - Superset supports workspace presets (startup scripts, env setup). Add
     preset definitions in forgemux (`forgemux.toml` or similar) that `fmux`
     can apply when creating sessions (install deps, run setup, seed env vars).

8. **Port discovery & service links**
   - Superset tracks active ports per worktree and surfaces them in the UI.
     Forgemux could add port scanning per session (opt-in) and show quick links
     in the dashboard, improving the "agent started a dev server" workflow.

9. **Fast attach + attach-only semantics**
   - Superset's daemon work emphasizes fast attach when a session already
     exists, with cold restore as a fallback. Forgemux can adopt similar
     semantics so `fmux attach` never blocks on rehydration logic when a tmux
     session is alive.

10. **Workspace-scoped filesystem isolation**
    - Follow Superset's "no global files" rule: keep all integration artifacts
      in `~/.forgemux[-{workspace}]/` so dev/prod or multiple instances do not
      conflict.

---

## 4. Immediate Next Experiments (Low Effort)

1. **Add `forged` hook interface** that accepts JSON events over a local socket
   or file, then create wrapper scripts for Claude + Codex to emit those events.
2. **Emit session env vars** for every tmux session and log them into the
   transcript header for easy debugging.
3. **Dashboard session inventory** showing idle/active status and allowing kill
   actions (even without full diff viewer).

These are small, high-leverage steps that borrow Superset's proven ergonomic
patterns without requiring forgemux to become a full desktop app.
