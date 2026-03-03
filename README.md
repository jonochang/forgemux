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

## Quick Start (Local Edge)

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

## Install (NixOS / Nix)

One-line install (flake):

```sh
nix profile install github:jonochang/forgemux
```

Or run without installing:

```sh
nix run github:jonochang/forgemux -- --help
```

Non-flake install (uses `default.nix` / `package.nix`):

```sh
nix-env -f . -iA forgemux
```

NixOS config (flake):

```nix
{
  inputs.forgemux.url = "github:jonochang/forgemux";
  outputs = { self, nixpkgs, forgemux, ... }: {
    nixosConfigurations.myhost = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        ({ pkgs, ... }: {
          environment.systemPackages = [
            forgemux.packages.${pkgs.system}.default
          ];
        })
      ];
    };
  };
}
```

## Quick Start (Hub + Dashboard)

## Configure (Local Dev)

Interactive config generation (recommended):

```sh
forgehub configure
forged configure
```

Or run a combined flow:

```sh
fmux configure
```

Non-interactive defaults (for automation):

```sh
forgehub configure --non-interactive
forged configure --non-interactive
```

1. Run the hub:

```sh
forgehub --bind 127.0.0.1:8080 --config ./.forgemux-hub.toml run
```

2. Run the edge with hub registration:

```sh
forged run --data-dir ./.forgemux --bind 127.0.0.1:9090 --config ./forged.toml
```

3. Start a session via the hub:

```sh
fmux --hub http://127.0.0.1:8080 start --agent claude --model sonnet --repo .
```

4. Open the dashboard:

```sh
open http://127.0.0.1:8080
```

The dashboard shows live sessions and allows browser attach.

## Auth Tokens (Optional)

If you set tokens, pass them via `--token` or `Authorization: Bearer`.

Example hub config (`./.forgemux-hub.toml`):

```toml
data_dir = "./.forgemux-hub"
tokens = ["dev-token"]
```

Example edge config (`./forged.toml`):

```toml
data_dir = "./.forgemux"
api_tokens = ["dev-token"]
```

CLI usage:

```sh
fmux --hub http://127.0.0.1:8080 --token dev-token ls
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

## Useful Commands

```sh
# Show usage for a session (returns zero until collectors are implemented)
fmux --edge http://127.0.0.1:9090 usage S-xxxxxxx

# Drain the edge (stop new sessions)
forged drain

# Metrics endpoints (Prometheus format)
curl http://127.0.0.1:9090/metrics
curl http://127.0.0.1:8080/metrics
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
