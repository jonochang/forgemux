# stereOS Review vs Forgemux

Date: 2026-03-02

## Scope
This note reviews `/home/jonochang/lib/stereOS` and compares it with Forgemux (as currently described in `docs/specs/` and `README.md`). It closes with concrete idea seeds for Forgemux.

## Snapshot: stereOS
stereOS positions itself as a hardened, minimal Linux OS purpose-built for AI agents, delivered as prebuilt VM images (“mixtapes”). Key traits from the repo:

- NixOS-based system built via flakes, with modular NixOS configuration and reproducible image formats.
- Mixtape concept: base OS plus a specific agent CLI package (OpenCode, Claude Code, Gemini CLI), with a “full” mix that composes all agents.
- Two-role user model: `admin` (wheel + full access) and `agent` (restricted shell, no sudo, no nix tooling).
- Control-plane daemons: `stereosd` (vsock/TCP control plane, secrets injection, shared mounts, lifecycle) and `agentd` (agent process management via tmux or gVisor sandboxes).
- Security posture: restricted PATH for the agent, no nix daemon access for the agent, SSH hardening, tmpfs for `/tmp`, firewall defaults.
- Boot-time optimization targeting sub‑3s boot for VM workloads with direct-kernel artifacts (bypass UEFI/GRUB), systemd initrd, and disabling unneeded services.
- Multiple image outputs: raw EFI, QCOW2, and kernel artifacts; “dist” builds include compressed artifacts plus a manifest with checksums.

## Snapshot: Forgemux
Forgemux is a Rust platform for durable, observable sessions with a tmux-backed edge, optional hub, and a lightweight dashboard. Its roadmap focuses on:

- Phase 0: Durable sessions + accurate state detection (including WaitingInput).
- Phase 1: Notifications on state transitions.
- Phase 2: Hub for multi-node aggregation.
- Phase 3: Browser/mobile attach with reliable stream protocol.

Forgemux emphasizes protocol correctness, state detection, and durability over OS-level isolation or VM packaging.

## Comparison (stereOS vs Forgemux)

## Product focus
stereOS is an operating-system distribution and image pipeline built around AI agent execution, with strong isolation and boot-time optimization. Forgemux is a session orchestration and observability layer that assumes an existing host OS and focuses on reliable state and attach semantics.

## Isolation and safety
stereOS bakes isolation into the OS: agent user has a curated PATH, no sudo, and no Nix daemon access; gVisor is available for sandboxed execution; secrets are injected via a root-owned control plane. Forgemux does not yet describe OS-level isolation, though it can wrap sessions with sidecars or runners in future phases.

## Control plane and lifecycle
stereOS includes a baked-in control plane (`stereosd` + `agentd`) that handles boot-time coordination, secret injection, shared mounts, and process management. Forgemux’s control plane is application-layer: state detection, eventing, and attach/detach semantics, with a hub model for multi-node aggregation.

## Distribution and deployment
stereOS ships prebuilt VM images (raw EFI, QCOW2, kernel artifacts) optimized for aarch64 virtualization frameworks, with a “dist” artifact bundle and checksums. Forgemux is deployed as binaries/services on existing hosts and expects the user to manage OS images/VMs separately.

## Extensibility and agent packaging
stereOS treats agent CLIs as “mixtape” variants that extend the agent’s PATH via Nix packages; it currently supports OpenCode, Claude Code, and Gemini CLI. Forgemux is agent-agnostic and focuses on session durability and monitoring; it can run any CLI but does not provide baked-in OS images or prepackaged agent stacks.

## Developer workflow
stereOS emphasizes Nix-based builds and reproducible images, with helper scripts for QEMU and direct-kernel boot. Forgemux emphasizes tmux/session semantics and a lightweight dashboard; it does not prescribe an OS build toolchain.

## Idea Seeds for Forgemux
Each idea is tagged with a suggested phase fit or prerequisite.

## 1. “Runner” abstraction for VM-backed sessions
Why: stereOS treats isolation as a first-class OS concern and provides VM images with hardened defaults. This is a useful complement to Forgemux’s session durability focus.
Fit: Phase 1 or Phase 2.
Notes: Define a runner interface (`local`, `docker`, `vm`) and allow per-session runner selection. A VM runner could integrate with stereOS-like images or similar hardened VM baselines.

## 2. Control-plane hooks for secrets injection
Why: stereOS injects secrets at boot via a control plane (vsock/TCP) rather than baking them into images.
Fit: Phase 2.
Notes: Add a secure secret delivery path in Forgemux’s hub protocol so session environments can be ephemeral and secrets never hit disk.

## 3. “Session OS profile” concept
Why: stereOS uses mixtapes to define a base OS + agent package set. This is useful for multi-agent standardization.
Fit: Phase 2 or Phase 3.
Notes: Forgemux could define a profile schema that maps to a runner + packages + env vars. Even for local sessions, this helps reduce drift across many hosts.

## 4. Direct-kernel boot as a latency play
Why: stereOS shows that avoiding UEFI/GRUB and trimming services can materially reduce boot time for VM agents.
Fit: Phase 3 (if VM runner exists).
Notes: If Forgemux adds VM runners, consider supporting direct-kernel boot paths for fast session spin-up.

## 5. Admin vs agent role separation
Why: stereOS codifies admin/agent separation and denies agent sudo explicitly, reducing blast radius.
Fit: Phase 1 or Phase 2.
Notes: Forgemux can mirror this with a privileged “operator” control plane and least-privilege agent execution wrappers (even outside a VM).

## 6. Dist artifact + manifest conventions
Why: stereOS packages images with a manifest containing checksums and sizes, simplifying downstream automation.
Fit: Phase 2.
Notes: If Forgemux ships packaged runners or images, adopt a manifest format for integrity and reproducible downloads.

## 7. gVisor-compatible sandbox mode
Why: stereOS includes gVisor runtime support for sandboxed agents with admin introspection.
Fit: Phase 2.
Notes: A runner mode that uses gVisor or similar container sandboxing could provide a middle ground between local and full VM isolation.

## 8. VM-first developer ergonomics
Why: stereOS provides a dedicated VM launcher with defaults for SSH, vsock, and boot mode.
Fit: Phase 2 or Phase 3.
Notes: Forgemux could ship a “dev harness” for VM-backed sessions to simplify onboarding to isolated environments.

## Strategic takeaways
- stereOS’s main differentiation is OS-level hardening + reproducible, VM-friendly artifacts for running agents. It emphasizes isolation, boot speed, and a strong control plane inside the guest.
- Forgemux’s differentiation is session durability and correctness across many nodes. The two can be complementary: Forgemux as the orchestration plane on top of hardened VM runners.
- If Forgemux adds a runner abstraction, stereOS-like images become a natural backend for “safe” or “isolated” sessions while keeping Forgemux’s core focus on state and observability.

## Proposed next steps (low effort)
- Draft a short `docs/research` note on a runner abstraction (`local`/`container`/`vm`) and how it fits the existing edge/hub model.
- Define a minimal secret-injection interface for future hub protocol extensions (even if not implemented yet).
- Add a “session profile” sketch to `docs/specs/` that maps profile names to runner + env + package set.
