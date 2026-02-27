# Research: Beehive Deep Dive + Implementation Ideas for Forgemux

**Date:** February 27, 2026
**Based on:** Deep code review of Beehive (`/home/jonochang/lib/beehive`) and Forgemux (`/home/jonochang/lib/jc/forgemux`).
**Complements:** `beehive.md` (surface-level comparison). This document focuses on implementation patterns, architectural contrasts, and concrete code-level ideas.

---

## 1. Architectural Contrast: Two Philosophies

| Dimension | Beehive | Forgemux |
|-----------|---------|----------|
| Runtime | Single-process desktop app (Tauri + Rust) | Multi-process distributed system (edge daemon + hub + CLI) |
| PTY substrate | `portable-pty` crate, direct master/slave FDs | tmux sessions (subprocess orchestration) |
| State model | Per-hive JSON files, React in-memory runtime | Per-session JSON with optimistic concurrency, versioned records |
| Concurrency | `Arc<Mutex<PtyManager>>` + per-PTY background I/O threads | `tokio` async runtime with `Arc<Mutex<SessionService>>` |
| Networking | None (local only) | HTTP REST + WebSocket + hub relay + planned gRPC |
| Session durability | PTYs die with app exit | Sessions survive daemon restarts (tmux-backed) |
| Agent awareness | Generic (any command via custom buttons) | Deep (JSONL log watching, prompt pattern detection, state machine) |
| Auth | Delegates to git/gh CLI | API tokens, pairing flow, TLS/mTLS (partial) |
| Distribution | Signed macOS .app + .dmg, auto-updater | Binary crates, no packaging yet |

**Key insight:** Beehive optimizes for the single-developer, single-machine experience with zero configuration. Forgemux optimizes for durability, observability, and multi-node orchestration at the cost of setup complexity. The gap worth closing is Forgemux's single-node UX.

---

## 2. PTY Management: Direct FDs vs tmux Subprocess

### Beehive's Approach

Beehive creates PTYs via `portable-pty`:

```rust
// pty.rs - Direct PTY creation
let pair = native_pty_system().openpty(PtySize { rows, cols, .. })?;
let child = pair.slave.spawn_command(cmd)?;
let master = Arc::new(Mutex::new(pair.master));
let writer = master.lock().try_clone_writer()?;

// Background I/O thread per session
tokio::task::spawn_blocking(move || {
    let mut reader = master.lock().try_clone_reader()?;
    let mut buf = [0u8; 4096];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 { break; }
        app.emit(&format!("pty-output-{session_id}"), &buf[..n])?;
    }
});
```

Advantages:
- Zero external dependencies (no tmux required)
- Direct byte-level control over PTY I/O
- Precise timing (no shell-based timestamp injection)
- Resize is synchronous (`master.resize()`)

### Forgemux's Approach

Forgemux wraps tmux:

```rust
// tmux session creation
tmux new-session -d -s {id} -x 200 -y 50
tmux send-keys -t {id} "{agent_cmd}" Enter

// Capture via pipe-pane
tmux pipe-pane -o -t {id} "awk '{ print strftime(...) }' >> transcript.log"
```

Advantages:
- Sessions survive daemon restarts and SSH disconnects
- Built-in detach/attach semantics
- Multi-client attach for free
- Mature window/pane management

### Idea: Hybrid PTY Layer

Forgemux could offer a **direct-PTY mode** for the local dashboard (Phase 3+) while keeping tmux for durable remote sessions:

```
Local dashboard attach  →  Direct PTY relay (low latency, no tmux overhead)
SSH attach              →  tmux attach (durable, survives disconnects)
Browser attach          →  WebSocket → tmux capture (current design)
```

This would give Beehive-like responsiveness for local users while preserving durability for remote use. The `SessionService` could abstract over both backends via a `PtyBackend` trait:

```rust
trait PtyBackend: Send + Sync {
    async fn create(&self, id: &SessionId, cmd: &str, cwd: &Path) -> Result<()>;
    async fn write(&self, id: &SessionId, data: &[u8]) -> Result<()>;
    async fn read(&self, id: &SessionId) -> Result<Vec<u8>>;
    async fn resize(&self, id: &SessionId, rows: u16, cols: u16) -> Result<()>;
    async fn destroy(&self, id: &SessionId) -> Result<()>;
    fn is_durable(&self) -> bool;
}

struct TmuxBackend { runner: TmuxRunner }
struct DirectPtyBackend { sessions: Arc<Mutex<HashMap<SessionId, PtySession>>> }
```

---

## 3. Terminal Rendering: Hidden-Display Pattern

### Beehive's Pattern

Beehive renders ALL open combs simultaneously, using CSS `display: none` to hide inactive ones. xterm.js instances stay alive in the background:

```tsx
// MainLayout.tsx - All combs render, only active one visible
{openedCombs.map(comb => (
    <div style={{ display: comb.id === activeCombId ? "block" : "none" }}>
        <WorkspaceGrid panes={panesByComb.get(comb.id)} />
    </div>
))}
```

This means:
- Terminal state (scrollback, cursor position, running processes) survives tab switches
- No reconnection lag when switching combs
- Memory cost is bounded by open comb count

### Idea for Forgemux Dashboard

The Forgemux dashboard (Phase 3) could adopt this pattern. When a user has multiple sessions open in the browser:

1. Keep WebSocket connections alive for all viewed sessions
2. Render xterm.js instances for each, hide inactive ones
3. On tab switch, just toggle visibility (instant)
4. Add a configurable limit (e.g., max 8 live connections) with LRU eviction

This avoids the common pattern of destroying and recreating terminal components on navigation, which causes reconnection delays and lost scrollback.

---

## 4. Per-Entity Custom Commands (Agent Buttons)

### Beehive's Implementation

Each Hive has a `custom_buttons` array:

```rust
struct CustomButton {
    label: String,   // "Claude Code", "Copilot", "Aider"
    cmd: String,     // "/usr/bin/claude", "aider", etc.
}
```

Users configure these per-repo, and clicking a button spawns an agent pane with that command in the comb's directory.

### Idea: Session Templates for Forgemux

Forgemux could generalize this into **session templates** stored in the forged config or per-repo `.forgemux.toml`:

```toml
# .forgemux.toml (per-repo) or forged.toml (global)
[templates.review]
agent = "claude"
model = "sonnet"
policy = "standard"
args = ["--dangerously-skip-permissions"]
system_prompt = "You are reviewing code for security issues."

[templates.implement]
agent = "claude"
model = "opus"
policy = "high_memory"

[templates.test-writer]
agent = "codex"
model = "o3"
```

CLI usage:
```bash
fmux start --template review
fmux start --template implement --branch feature/auth
```

Dashboard: Render template buttons per-repo (like Beehive's custom buttons) so users can one-click spawn typed sessions.

---

## 5. Workspace Duplication / Session Fork

### Beehive's Comb Copy

Beehive duplicates an entire workspace directory (recursive copy), preserving uncommitted changes:

```rust
// hive.rs - Copy comb
fn copy_comb(source_path: &str, dest_path: &str) -> Result<()> {
    // Full recursive directory copy
    // Preserves working tree state, staged changes, untracked files
    // New comb gets new UUID, new name, same branch
}
```

### Idea: `fmux fork` with Worktree Snapshot

Since Forgemux uses git worktrees, forking is cheaper than full directory copy:

```bash
fmux fork S-a1b2c3d4 --branch experiment/v2
```

Implementation:
1. Create new worktree from the source session's current commit (not branch HEAD)
2. Copy uncommitted changes via `git stash` + `git stash apply` in new worktree
3. Start a new session in the new worktree
4. Link the sessions in metadata (parent_session field) for traceability

This is more powerful than Beehive's approach because:
- Git worktrees share the object store (disk-efficient)
- The fork relationship is tracked
- Foreman could use forks to run parallel experiments

---

## 6. State Persistence Patterns

### Beehive: Simple JSON + Debounced Writes

```typescript
// Debounced layout save (500ms)
const debouncedSave = useMemo(
    () => debounce((panes) => invoke("save_pane_layout", { panes }), 500),
    []
);
```

Beehive writes state frequently but debounces to avoid I/O storms. State files are simple, human-readable JSON.

### Forgemux: Versioned Records + Optimistic Concurrency

```rust
pub fn save_checked(&self, record: &SessionRecord, expected_version: u64) -> Result<()> {
    let current = self.load(&record.id)?;
    if current.version != expected_version {
        return Err("version conflict");
    }
    record.version = expected_version + 1;
    self.save(record)
}
```

### Idea: Adopt Debounced Persistence for Hot-Path State

Forgemux's state polling loop updates session records every poll cycle. For high-frequency state (like `last_activity_at`), debouncing would reduce disk I/O:

```rust
struct DebouncedStore {
    inner: SessionStore,
    pending: HashMap<SessionId, (SessionRecord, Instant)>,
    flush_interval: Duration,  // e.g., 500ms
}

impl DebouncedStore {
    fn save_debounced(&mut self, record: SessionRecord) {
        self.pending.insert(record.id.clone(), (record, Instant::now()));
    }

    async fn flush(&mut self) {
        let now = Instant::now();
        let ready: Vec<_> = self.pending.iter()
            .filter(|(_, (_, t))| now - *t >= self.flush_interval)
            .map(|(id, _)| id.clone())
            .collect();
        for id in ready {
            if let Some((record, _)) = self.pending.remove(&id) {
                self.inner.save(&record)?;
            }
        }
    }
}
```

Keep `save_checked` for state transitions (Running -> Idle) but use debounced writes for activity timestamps. This reduces disk I/O from O(sessions * poll_rate) to O(sessions * flush_rate).

---

## 7. Focus Tracking and Session Priority

### Beehive's Focus Model

Beehive tracks the last-focused pane per comb:

```typescript
interface HiveRuntime {
    focusedPaneByComb: Map<string, string>;  // combId -> paneId
}
```

When switching combs, focus restores automatically to the last-active pane. This is a small UX detail that makes navigation feel seamless.

### Idea: Focus/Attention Signals for Forgemux

Forgemux could track which session the user is currently viewing (attached to):

```rust
pub struct SessionRecord {
    // ... existing fields ...
    pub attached_clients: u32,      // How many clients are attached
    pub last_viewed_at: DateTime<Utc>,  // Last time a human looked at this
}
```

Benefits:
- **Notification suppression**: Don't send desktop notifications for sessions the user is actively viewing
- **Foreman priority**: Foreman knows which sessions are unattended and may need intervention
- **Dashboard sorting**: Recently-viewed sessions float to top
- **Resource hints**: Edge could deprioritize snapshot frequency for unattended sessions

---

## 8. Preflight/Onboarding: Beehive's Guided Flow

### Beehive's Setup Flow

```
App Launch
  └── Preflight Screen
      ├── Check: git installed?
      ├── Check: gh installed?
      ├── Check: gh auth status?
      └── All pass → Setup Screen
          └── Directory picker with autocomplete
              └── beehive_dir saved to ~/.beehive/config.json
                  └── Main app
```

Each check shows pass/fail with actionable remediation instructions. The directory picker uses filesystem autocomplete for a polished feel.

### Idea: `fmux init` Interactive Setup

Forgemux has `fmux doctor` for diagnostics but no guided first-run. Add an interactive init:

```bash
$ fmux init

Forgemux Setup
==============

Checking dependencies...
  [ok] tmux 3.4
  [ok] git 2.43.0
  [ok] gh 2.44.0 (authenticated as jonochang)
  [!!] claude not found in PATH
       Install: https://docs.anthropic.com/claude-code/install

Configure defaults:
  Hub URL [none]: https://hub.internal:9443
  Default workspace root [~/workspaces]: ~/dev/workspaces
  Default agent [claude]:
  Default model [sonnet]:

Written to ~/.config/forgemux/config.toml

Run `fmux start` to create your first session.
```

Key details from Beehive to adopt:
- **Fail fast with remediation**: Don't just say "missing", say how to install
- **Autocomplete for paths**: If building a TUI, use path completion
- **Minimal required config**: Only ask what's needed, everything else has defaults

---

## 9. Build, Packaging, and Distribution

### Beehive's Distribution Model

- Signed macOS `.app` bundle via Tauri
- `.dmg` installer
- Auto-update via Tauri updater plugin + GitHub Releases
- Hardened runtime with explicit PATH injection
- `build.sh` with preflight checks, --check, --dev, --release modes

### Idea: Forgemux Distribution Strategy

Forgemux currently has no packaging story. Drawing from Beehive:

1. **Self-contained binary**: `cargo build --release` already produces static binaries. Add a `install.sh` that:
   - Downloads the right binary for the platform
   - Places it in `/usr/local/bin/` (or `~/.local/bin/`)
   - Runs `fmux init` on first launch

2. **Version checking**: Beehive's Tauri updater checks for updates on launch. Forgemux could add:
   ```bash
   fmux update  # Check for and install updates
   fmux version  # Show version + check if update available
   ```

3. **Build script**: A `build.sh` similar to Beehive's with:
   - `./build.sh --check` for CI (cargo check + clippy + test)
   - `./build.sh --release` for tagged releases
   - Cross-compilation targets (Linux x86_64, aarch64, macOS)

---

## 10. What Beehive Does NOT Have (Forgemux Advantages)

These are areas where Forgemux is already stronger and should not regress:

| Capability | Beehive | Forgemux |
|------------|---------|----------|
| Session durability | PTYs die with app | tmux sessions survive everything |
| Multi-client attach | Not supported | Free via tmux + WebSocket relay |
| State detection | None (raw terminal only) | State machine + JSONL log watching |
| Transcript capture | None | Timestamped transcript files |
| Usage tracking | None | Token/cost parsing from agent logs |
| Policy enforcement | None | cgroup limits, network isolation (planned) |
| Multi-node | Single machine only | Edge/hub architecture |
| Notifications | None | Desktop, webhook, command hooks |
| Foreman/meta-agent | None | Scaffolded supervisor agent |
| Encryption | None | AES-256-GCM stream encryption |
| Reliable streams | N/A (local only) | Event ring + snapshots + input dedup |

---

## 11. Concrete Implementation Priorities (Derived from This Analysis)

### High Value, Low Effort

1. **Session templates** (Section 4): Add `[templates]` to forged.toml and `.forgemux.toml`. Wire into `fmux start --template`. Small config parsing change + CLI flag.

2. **`fmux init` guided setup** (Section 8): Interactive first-run wizard. Generates config.toml with defaults. Runs doctor checks inline.

3. **Attention tracking** (Section 7): Add `attached_clients` and `last_viewed_at` to SessionRecord. Update on attach/detach. Use for notification suppression.

### Medium Value, Medium Effort

4. **Debounced activity persistence** (Section 6): Separate hot-path writes (activity timestamps) from cold-path writes (state transitions). Reduces I/O under load.

5. **Session fork** (Section 5): `fmux fork` with worktree snapshot. Requires worktree creation + stash transfer + metadata linking.

6. **Hidden-display terminal pattern** (Section 3): For dashboard Phase 3. Keep WebSocket connections alive for background sessions, toggle visibility on tab switch.

### High Value, High Effort

7. **PtyBackend trait abstraction** (Section 2): Abstract over tmux vs direct PTY. Enables local dashboard with Beehive-like responsiveness while keeping tmux for remote. Major refactor of SessionService.

8. **Build/release pipeline** (Section 9): install.sh, cross-compilation, version checking, update command. Infrastructure work.

---

## 12. Open Design Questions

1. **Should session templates live in forged.toml (global) or per-repo `.forgemux.toml` (local)?**
   Both, with local overriding global. But this introduces config discovery logic.

2. **Is a PtyBackend abstraction worth the complexity for Phase 3?**
   Only if the local dashboard is a priority. If browser-only, tmux capture is sufficient.

3. **Should `fmux fork` preserve the source session's transcript?**
   Probably yes (copy or symlink to parent transcript), but it grows storage. Could be opt-in.

4. **How should attention tracking interact with Foreman?**
   Foreman should know which sessions are unattended. Should Foreman auto-escalate intervention level for sessions with `attached_clients == 0`?

---

## Summary

The first `beehive.md` identified workflow and UX gaps. This deeper dive reveals **implementation-level patterns** worth adopting:

- **Direct PTY management** as an alternative backend for local responsiveness
- **Hidden-display rendering** for the dashboard to avoid reconnection churn
- **Session templates** as the generalization of Beehive's custom buttons
- **Debounced persistence** for high-frequency state updates
- **Session forking** via git worktree snapshots
- **Attention signals** for smarter notifications and Foreman behavior
- **Guided init flow** replacing bare config file editing

The core takeaway: Beehive's strength is in making the single-developer experience feel effortless. Forgemux can absorb these patterns without compromising its distributed, durable architecture.
