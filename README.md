# forgemux

Forgemux is a Rust-based platform for managing durable, observable AI coding agent sessions.
It provides a tmux-backed session layer for local/edge execution with optional hub aggregation
and a lightweight dashboard.

## Status

This repository is an early-stage scaffold based on the specs in `docs/specs/`.
It includes:

- `fmux` CLI: start/attach/list sessions, watch state, foreman scaffolding
- `forged` edge service: tmux lifecycle + transcript capture + state detection
- `forgehub` hub service: basic session aggregation + dashboard placeholder
- Nix flake dev shell with Rust toolchain and common build tools

## Development

Enter the dev shell:

```sh
nix develop
```

Generate lockfile and run tests:

```sh
cargo generate-lockfile
cargo test
```

## Layout

- `crates/forgemux-core`: shared types, session state machine, repo root detection
- `crates/fmux`: CLI
- `crates/forged`: edge daemon
- `crates/forgehub`: hub server
- `dashboard/`: static dashboard placeholder

## Notes

This is a scaffold that covers Phase 0–5 at a minimal functional level. Some components
(e.g., hub gRPC, PTY websocket bridge, real foreman reports, webhook notifications) are
placeholders and need deeper implementation.
