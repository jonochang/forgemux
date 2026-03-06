# Forgemux — New Ideas

**Date:** 2026-03-02
**Sources:** `docs/research/competitors/openfang.md`, `docs/research/competitors/stereos.md`

Ideas are grouped by theme, tagged with a phase fit, and rated by leverage (value vs effort).

---

## 1. Runner Abstraction (`local` / `container` / `vm`)

**Sources:** stereOS idea 1, OpenFang idea 1
**Phase fit:** Phase 1–2
**Leverage:** High

Define a `runner` interface with at least three modes:

| Runner | Isolation | Fit |
|--------|-----------|-----|
| `local` | None (current behavior) | Dev, trusted workloads |
| `container` | Docker / OCI container | Multi-tenant, network-isolated |
| `vm` | Full VM (stereOS-like images, gVisor) | High-risk or untrusted agents |

Per-session runner selection via `fmux start --runner vm` or config. The edge daemon stays the same; runner selection is a launch-time wrapper around the tmux session startup.

This is the single highest-leverage idea from both reviews: it differentiates Forgemux from substrate-only tools and makes security a first-class option without requiring it everywhere.

**Notes:**
- Start with a `container` runner backed by `docker run` + a bind-mounted tmux socket.
- The `vm` runner can integrate with stereOS images (raw EFI / QCOW2) or similar hardened baselines.
- Keep the runner interface minimal: `start(session) -> pid`, `stop(pid)`, `is_alive(pid)`.

---

## 2. Session Security Profiles

**Sources:** OpenFang idea 1, stereOS ideas 3 and 5
**Phase fit:** Phase 1–2
**Leverage:** High

A named profile bundles runner + environment constraints + network policy. Example:

```toml
[profiles.safe]
runner = "container"
network = "none"
env_scrub = true
secrets_source = "hub"

[profiles.trusted]
runner = "local"
```

Sessions inherit a profile from workspace config or can specify it at start. Even a simple two-profile model (`trusted` / `safe`) gives Forgemux a concrete security story — currently absent.

**Notes:**
- Mirrors stereOS's admin/agent role separation: the "safe" profile restricts PATH, disables sudo, and enforces no-network by default.
- Aligns with OpenFang's security tiering messaging.

---

## 3. Secure Secrets Injection via Hub

**Sources:** stereOS idea 2
**Phase fit:** Phase 2
**Leverage:** Medium

Add a secrets delivery path to the hub protocol so sessions can receive credentials at start time without baking them into images, config files, or environment variables on disk.

Design sketch:
- Hub holds an encrypted secret store (or proxies to a backend like Vault/1Password CLI).
- On session start, hub pushes secrets to the edge over the existing auth channel.
- Edge injects secrets into the tmux session environment via `tmux set-environment` before agent launch.
- Secrets are never written to disk; they exist only in the tmux environment for the session lifetime.

**Notes:**
- Requires the hub-to-edge auth path already in place (CC-3 from the implementation plan).
- Minimal v1 could just be hub-side env var interpolation from a local secrets file.

---

## 4. Session Bundle Format ("Hand" equivalent)

**Sources:** OpenFang idea 3
**Phase fit:** Phase 1
**Leverage:** Medium

Define a minimal `forgemux-bundle` spec — a directory or TOML that packages everything needed to reproduce a session type:

```
my-bundle/
  bundle.toml       # entry command, agent, model, runner, env, required secrets
  system.md         # system prompt or instruction file
  hooks/            # lifecycle scripts (on-start, on-idle, on-stop)
```

`fmux start --bundle ./my-bundle` launches the session from the spec. This is the "packaged autonomous behavior" narrative without needing a marketplace.

**Notes:**
- Aligns naturally with the workspace `repos` + `templates` config already sketched in the implementation plan.
- Bundles can be shared as tarballs or git repos — a lightweight distribution story.

---

## 5. Workflow and Trigger Primitives

**Sources:** OpenFang idea 2
**Phase fit:** Phase 2
**Leverage:** Medium

Expose session state events as triggers for automation:

```toml
[[triggers]]
event = "session.state == WaitingInput"
action = "notify slack"

[[triggers]]
event = "session.state == Terminated AND session.exit_code != 0"
action = "spawn ./retry.sh"
```

`fmux workflow` could read a workflow file and subscribe to the event stream, executing actions on matching transitions.

**Notes:**
- The event model already exists (state machine + broadcast channel). Exposing it externally is the main work.
- Keep the core lean: make actions external shell commands or webhooks rather than built-in integrations.

---

## 6. Operator/Agent Role Separation

**Sources:** stereOS idea 5
**Phase fit:** Phase 1
**Leverage:** Medium

Formalize the distinction between the human operator (the forged/forgehub process, which has full host access) and the agent process (running inside the tmux session, which should be least-privilege).

Concretely:
- Agent user has a restricted PATH (no `sudo`, no package managers, no access to `~/.ssh`).
- Agent's working directory is scoped to the repo root.
- `fmux start` accepts a `--restrict` flag that applies a curated `PATH` and `HOME` override to the tmux session.

**Notes:**
- Achievable today for the `local` runner without a VM: just set `PATH` and `HOME` in the tmux launch env.
- Becomes more meaningful with the `container` runner where cgroups and uid mapping add real enforcement.

---

## 7. Append-Only Audit Log

**Sources:** OpenFang idea 8
**Phase fit:** Phase 1–2
**Leverage:** Medium

Forgemux already has `replay.jsonl` per session. Extend this with:
- **Tamper evidence:** SHA-256 chain — each event includes a `prev_hash` field, enabling offline verification.
- **Hub-side aggregation:** Hub collects replay logs from edges into a central audit store.
- **Structured export:** `fmux audit <session-id>` dumps the verified event chain.

**Notes:**
- The hash chain is low effort on top of the existing JSONL format — just add a hash field at write time.
- Even without verification tooling, the field makes the audit story credible.

---

## 8. TUI Session Dashboard

**Sources:** OpenFang idea 5
**Phase fit:** Phase 1
**Leverage:** Medium

A thin `ratatui`-based TUI that shows the session list, states, and allows attach/detach without a browser. Covers the "quick local check" workflow that the web dashboard is too heavy for.

Minimum viable feature set:
- Session list with state color-coding (Running / WaitingInput / Errored)
- Arrow keys to select, `Enter` to attach
- `k` to kill, `q` to quit
- Reads from `forged`'s HTTP API (no special IPC)

**Notes:**
- `fmux tui` as a subcommand, or integrated into `fmux sessions` with a `--watch` flag.
- This is the highest-leverage UX addition for terminal-native users who don't want to open a browser.

---

## 9. Desktop App (Tauri Wrapper)

**Sources:** OpenFang idea 6
**Phase fit:** Phase 3
**Leverage:** Low–Medium

Wrap the existing hub web dashboard in a Tauri desktop app for a native install story on macOS/Linux. The hub web UI is already self-contained; Tauri provides the OS frame, system tray integration, and auto-launch.

**Notes:**
- Deferred to Phase 3 because it requires the web UI to be stable and polished first.
- Low effort once the web UI is complete — Tauri's web view renders the existing SPA unchanged.

---

## 10. VM-First Dev Ergonomics (Harness)

**Sources:** stereOS ideas 4 and 8
**Phase fit:** Phase 2–3 (only if runner abstraction exists)
**Leverage:** Low (conditional)

If the VM runner is implemented, provide a dev harness for quick iteration:
- `fmux vm start` — boots a VM using a specified image, SSH waits for readiness, then attaches the edge.
- Supports direct-kernel boot for sub-3s startup (pass `kernel` + `initrd` directly to QEMU, bypassing UEFI).
- `fmux vm stop` — cleanly terminates the VM.

**Notes:**
- Blocked on runner abstraction (idea 1). Not independently useful without it.
- stereOS shows direct-kernel boot is non-trivial but achievable; the main gain is latency for iterative workloads.

---

## Summary Table

| # | Idea | Sources | Phase | Leverage |
|---|------|---------|-------|----------|
| 1 | Runner abstraction (local/container/vm) | stereOS, OpenFang | 1–2 | High |
| 2 | Session security profiles | OpenFang, stereOS | 1–2 | High |
| 3 | Secure secrets injection via hub | stereOS | 2 | Medium |
| 4 | Session bundle format | OpenFang | 1 | Medium |
| 5 | Workflow and trigger primitives | OpenFang | 2 | Medium |
| 6 | Operator/agent role separation | stereOS | 1 | Medium |
| 7 | Append-only audit log | OpenFang | 1–2 | Medium |
| 8 | TUI session dashboard | OpenFang | 1 | Medium |
| 9 | Desktop app (Tauri) | OpenFang | 3 | Low–Medium |
| 10 | VM-first dev ergonomics | stereOS | 2–3 | Low (conditional) |

---

## Strategic Note

The two competitors approach agent infrastructure from opposite directions:
- **OpenFang** builds down from UX: rich integrations, a marketplace, and packaged behaviors targeting end-users who want an all-in-one agent OS.
- **stereOS** builds up from the OS: hardened VM images and a minimal control plane targeting operators who want isolation first.

Forgemux sits between them — a session substrate that can grow either toward richer UX (TUI, bundles, triggers) or toward deeper isolation (runners, profiles, secrets). The runner abstraction and security profiles (ideas 1 and 2) are the highest-leverage additions because they simultaneously address the gap vs. both competitors while keeping the core lean.
