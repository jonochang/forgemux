# Changelog

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
