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

## Quick Start

Run the edge daemon:

```sh
forged run --data-dir ./.forgemux --bind 127.0.0.1:9090
```

Start a session in the current repo:

```sh
fmux --edge http://127.0.0.1:9090 start --agent claude --model sonnet --repo .
```

Start a session in a new git worktree on a new branch (default path):

```sh
fmux --edge http://127.0.0.1:9090 start --worktree --branch my-feature
```

Override the worktree path:

```sh
fmux --edge http://127.0.0.1:9090 start --worktree --branch my-feature --worktree-path /path/to/worktree
```

List and attach:

```sh
fmux --edge http://127.0.0.1:9090 ls
fmux attach S-xxxxxxx
```

## Hub Config

`fmux edges` and `forgehub` expect a config file (default `./.forgemux-hub.toml`) like:

```toml
data_dir = "./.forgemux-hub"

[[edges]]
id = "edge-01"
data_dir = "/path/to/edge/.forgemux"
ws_url = "ws://127.0.0.1:9090"
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
