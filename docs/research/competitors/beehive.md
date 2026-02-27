# Research: Beehive Comparison + Ideas for Forgemux

**Date:** February 27, 2026
**Based on:** Review of Beehive (`/home/jonochang/lib/beehive`) and Forgemux (`/home/jonochang/lib/jc/forgemux`).

This document summarizes how Beehive compares to Forgemux and extracts product and
implementation ideas that could inform Forgemux roadmap updates.

---

## 1. Snapshot: What Beehive Is

Beehive is a Tauri desktop app focused on orchestrating coding across isolated
Git workspaces with side‑by‑side terminals and agent panes. Key traits:

- **Desktop GUI** with a simple multi-screen flow (preflight → setup → repo list →
  workspace list → terminal grid).
- **Multi-repo management** with a lightweight data model (Hive = repo,
  Comb = workspace clone).
- **Persistent PTYs** with xterm.js panes and a flexible grid layout.
- **Git operations** driven by `git` + `gh` CLI.
- **Local filesystem as source of truth** with JSON state files.

Forgemux today is a CLI + daemon + hub architecture focused on durable tmux
sessions, observability, and multi-edge aggregation, with a placeholder
web dashboard.

---

## 2. Comparison: Beehive vs Forgemux

**Product focus**
- Beehive: Individual developer productivity in a GUI. Fast context switching
  between repos and workspaces.
- Forgemux: Durable, observable agent sessions across edge/hub, with
  multi-client attach and future automation (Foreman).

**Session model**
- Beehive: Local PTY per pane, UI-native layout, and strong emphasis on
  workspace cloning/branch selection.
- Forgemux: tmux-backed sessions with durable transcripts and state detection;
  worktree support exists but is CLI-driven.

**Workspace management**
- Beehive: Explicit concepts (Hive/Comb) and metadata stored per repo; easy
  create/delete flows with git clone/checkout.
- Forgemux: Worktree option on session start but no repo catalog or workspace
  list UI.

**UX & onboarding**
- Beehive: Preflight checks (git, gh, auth), guided setup, directory picker with
  autocomplete, and settings reset.
- Forgemux: CLI-based setup; no dedicated UX for dependency validation or
  “first run.”

**UI**
- Beehive: Grid of terminals/agent panes with layout persistence.
- Forgemux: tmux layout is powerful but opaque; dashboard is currently
  placeholder and does not expose layout or local multi-pane ergonomics.

---

## 3. Ideas for Forgemux (Derived from Beehive)

### 3.1 Workflow and UX

1. **“Preflight” checks as first-class**
   - Add `fmux doctor` (already recommended elsewhere) with explicit checks for
     `git`, `gh`, agent binaries, tmux, and auth. This mirrors Beehive’s preflight
     screen and reduces setup friction.

2. **Directory picker + worktree defaults**
   - Beehive’s directory autocomplete and beehive-dir setup suggests a CLI
     improvement: `fmux init` could propose a default workspace root and set it
     in config, avoiding repeated `--worktree-path` usage.

3. **Repo catalog (optional)**
   - A lightweight “repo registry” (like Beehive’s Hive list) could live in the
     CLI config or hub, enabling `fmux start --repo <alias>` and easy listing of
     existing workspaces. This avoids remembering long repo paths and standardizes
     worktree locations.

4. **Workspace list for a repo**
   - Beehive exposes Combs (clones/branches) per repo. Forgemux could add
     `fmux workspaces <repo>` with create/delete/list and a default path
     convention. This aligns with the “agent sessions per branch” workflow.

5. **Session copy / fork**
   - Beehive allows duplicating a comb (copy with uncommitted work). For
     Forgemux: add `fmux fork <session>` to create a new worktree and session
     from an existing one. Useful for experimentation without losing context.

### 3.2 UI and Dashboard

6. **Layout persistence surfaced in UI**
   - Beehive persists pane layout. Forgemux could store a `layout` metadata
     blob per session (even if tmux-driven) to let the dashboard render a
     meaningful session “shape” (e.g., grid, active panes, agent vs terminal).

7. **Explicit “pane roles”**
   - Beehive marks panes as TERM/AGENT. Forgemux already tracks agent type in
     session config; expose this as a pane role tag in the dashboard and session
     metadata for clarity when multiple panes exist.

8. **Local dashboard for edge-only mode**
   - Beehive proves local GUI is valuable even without a hub. Consider a
     lightweight local dashboard served by `forged` (single-node UI) that shows
     sessions, allows attach, and displays state. This would mirror Beehive’s
     simplicity while keeping Forgemux’s architecture.

### 3.3 Git and Workspace Operations

9. **Branch selection UX**
   - Beehive’s custom branch dropdown suggests a simpler CLI flow:
     `fmux start --worktree --branch <branch>` could autocomplete via `gh` or
     `git` (if configured). For the dashboard, a branch picker with search
     reduces mistakes.

10. **Repo verification before clone**
    - Beehive verifies repos via `gh` and `git ls-remote` before creating a hive.
      For Forgemux, preflight validation in `fmux start` could fail early with
      clearer errors and suggest `gh auth login` if needed.

### 3.4 State and Persistence

11. **Filesystem as source of truth**
    - Beehive’s state.json per hive is easy to inspect and recover. Forgemux
      already writes session records, but the “repo/workspace registry” (if
      adopted) should remain simple JSON in the edge data dir for durability.

12. **Settings reset / cleanup**
    - Beehive’s double-confirm reset is a good pattern. For Forgemux, consider
      `fmux reset` to clear CLI config, cache, and (optionally) local session
      metadata with a double-confirmation prompt.

---

## 4. Potential Roadmap Additions (Condensed)

- **Phase 0-1:** `fmux doctor` preflight checks (git/gh/tmux/agent/auth).
- **Phase 0-1:** `fmux init` to set a default workspace root for worktrees.
- **Phase 1-2:** Repo registry + workspace list (`fmux repo add/list`,
  `fmux workspaces list/create/delete`).
- **Phase 2-3:** Session fork (`fmux fork`) to copy a workspace + start a new
  session.
- **Phase 3:** Dashboard: display pane roles + layout metadata, add branch picker.
- **Phase 3-4:** Local edge dashboard mode for single-node users.

---

## 5. Open Questions to Validate

- Do we want Forgemux to remain strictly CLI-first, or should a local dashboard
  be positioned as a first-class single-node experience (like Beehive)?
- Is a repo registry compatible with the “stateless CLI” philosophy, or should
  it live in the hub for multi-edge deployments?
- Should workspaces be treated as first-class objects (like Beehive Combs)
  separate from sessions, or should they remain session-derived only?

---

## Summary

Beehive emphasizes local UX: preflight checks, guided setup, repo/workspace
management, and layout‑persistent terminals. Forgemux emphasizes durable agent
sessions, observability, and multi-edge infrastructure. The most transferable
ideas are in onboarding, workspace management, and lightweight UI affordances
(pane roles, layout metadata, local dashboard). These additions would improve
Forgemux’s single-node experience without changing the core architecture.
