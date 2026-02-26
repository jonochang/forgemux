use anyhow::Context;
use chrono::{DateTime, Utc};
use forgemux_core::{
    sort_sessions, AgentType, InterventionLevel, SessionManager, SessionRecord, SessionRole,
    SessionState, SessionStore, StateDetector, StateSignal,
};
use regex::Regex;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::SystemTime;

pub mod server;
pub mod checks;
pub mod stream;

pub trait CommandRunner: Send + Sync {
    fn run(&self, program: &str, args: &[String]) -> std::io::Result<Output>;
}

#[derive(Clone)]
pub struct OsCommandRunner;

impl CommandRunner for OsCommandRunner {
    fn run(&self, program: &str, args: &[String]) -> std::io::Result<Output> {
        Command::new(program).args(args).output()
    }
}

#[cfg(test)]
#[derive(Clone, Default)]
pub struct FakeRunner {
    calls: std::sync::Arc<std::sync::Mutex<Vec<Vec<String>>>>,
    pub should_fail: bool,
}

#[cfg(test)]
impl FakeRunner {
    pub fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().unwrap().clone()
    }
}

#[cfg(test)]
impl CommandRunner for FakeRunner {
    fn run(&self, program: &str, args: &[String]) -> std::io::Result<Output> {
        use std::os::unix::process::ExitStatusExt;
        let mut call = vec![program.to_string()];
        call.extend_from_slice(args);
        self.calls.lock().unwrap().push(call);
        let status = if self.should_fail {
            std::process::ExitStatus::from_raw(1)
        } else {
            std::process::ExitStatus::from_raw(0)
        };
        Ok(Output {
            status,
            stdout: Vec::new(),
            stderr: Vec::new(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub command: String,
    pub args: Vec<String>,
    pub prompt_patterns: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum NotificationHook {
    Desktop,
    Webhook { url: String, template: String },
    Command { program: String, args: Vec<String> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum NotificationKind {
    Desktop,
    Webhook,
    Command,
}

#[derive(Debug, Clone)]
pub struct NotificationConfig {
    pub on_waiting_input: Vec<NotificationHook>,
    pub on_error: Vec<NotificationHook>,
    pub on_idle_timeout: Vec<NotificationHook>,
    pub debounce_secs: i64,
}

#[derive(Debug, Clone)]
pub struct ForgedConfig {
    pub data_dir: PathBuf,
    pub tmux_bin: String,
    pub idle_threshold_secs: i64,
    pub waiting_threshold_secs: i64,
    pub agents: HashMap<AgentType, AgentConfig>,
    pub notifications: NotificationConfig,
    pub node_id: Option<String>,
    pub hub_url: Option<String>,
    pub advertise_addr: Option<String>,
    pub event_ring_capacity: usize,
    pub input_dedup_window: usize,
    pub snapshot_lines: i32,
    pub poll_interval_ms: u64,
}

impl ForgedConfig {
    pub fn default_with_data_dir(data_dir: PathBuf) -> Self {
        let mut agents = HashMap::new();
        agents.insert(
            AgentType::Claude,
            AgentConfig {
                command: "claude".to_string(),
                args: vec![],
                prompt_patterns: vec![r"(?m)^>\s*$".to_string()],
            },
        );
        agents.insert(
            AgentType::Codex,
            AgentConfig {
                command: "codex".to_string(),
                args: vec![],
                prompt_patterns: vec![r"(?m)^(?:> |\$ )".to_string()],
            },
        );

        Self {
            data_dir,
            tmux_bin: "tmux".to_string(),
            idle_threshold_secs: 60,
            waiting_threshold_secs: 15,
            agents,
            notifications: NotificationConfig {
                on_waiting_input: Vec::new(),
                on_error: Vec::new(),
                on_idle_timeout: Vec::new(),
                debounce_secs: 300,
            },
            node_id: None,
            hub_url: None,
            advertise_addr: None,
            event_ring_capacity: 512,
            input_dedup_window: 1000,
            snapshot_lines: 5000,
            poll_interval_ms: 250,
        }
    }

    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let data = fs::read_to_string(path)?;
        let file: ForgedConfigFile = toml::from_str(&data)?;
        let data_dir = file
            .data_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from("./.forgemux"));
        let mut config = Self::default_with_data_dir(data_dir);

        if let Some(tmux_bin) = file.tmux_bin {
            config.tmux_bin = tmux_bin;
        }
        if let Some(idle_threshold_secs) = file.idle_threshold_secs {
            config.idle_threshold_secs = idle_threshold_secs;
        }
        if let Some(waiting_threshold_secs) = file.waiting_threshold_secs {
            config.waiting_threshold_secs = waiting_threshold_secs;
        }
        if let Some(agents) = file.agents {
            for (name, agent) in agents {
                let agent_type = match name.as_str() {
                    "claude" => AgentType::Claude,
                    "codex" => AgentType::Codex,
                    _ => anyhow::bail!("unknown agent in config: {}", name),
                };
                let entry = config
                    .agents
                    .get_mut(&agent_type)
                    .expect("default agent config missing");
                if let Some(command) = agent.command {
                    entry.command = command;
                }
                if let Some(args) = agent.args {
                    entry.args = args;
                }
                if let Some(prompt_patterns) = agent.prompt_patterns {
                    entry.prompt_patterns = prompt_patterns;
                }
            }
        }
        if let Some(notifications) = file.notifications {
            config.notifications = notifications.into();
        }
        config.node_id = file.node_id;
        config.hub_url = file.hub_url;
        config.advertise_addr = file.advertise_addr;
        if let Some(event_ring_capacity) = file.event_ring_capacity {
            config.event_ring_capacity = event_ring_capacity;
        }
        if let Some(input_dedup_window) = file.input_dedup_window {
            config.input_dedup_window = input_dedup_window;
        }
        if let Some(snapshot_lines) = file.snapshot_lines {
            config.snapshot_lines = snapshot_lines;
        }
        if let Some(poll_interval_ms) = file.poll_interval_ms {
            config.poll_interval_ms = poll_interval_ms;
        }
        Ok(config)
    }
}

#[derive(Debug, serde::Deserialize)]
struct ForgedConfigFile {
    pub data_dir: Option<PathBuf>,
    pub tmux_bin: Option<String>,
    pub idle_threshold_secs: Option<i64>,
    pub waiting_threshold_secs: Option<i64>,
    pub agents: Option<HashMap<String, AgentFile>>,
    pub notifications: Option<NotificationConfigFile>,
    pub node_id: Option<String>,
    pub hub_url: Option<String>,
    pub advertise_addr: Option<String>,
    pub event_ring_capacity: Option<usize>,
    pub input_dedup_window: Option<usize>,
    pub snapshot_lines: Option<i32>,
    pub poll_interval_ms: Option<u64>,
}

#[derive(Debug, serde::Deserialize)]
struct AgentFile {
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub prompt_patterns: Option<Vec<String>>,
}

#[derive(Debug, serde::Deserialize)]
struct NotificationConfigFile {
    pub on_waiting_input: Option<Vec<NotificationHookFile>>,
    pub on_error: Option<Vec<NotificationHookFile>>,
    pub on_idle_timeout: Option<Vec<NotificationHookFile>>,
    pub debounce_secs: Option<i64>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum NotificationHookFile {
    Desktop,
    Webhook {
        url: String,
        template: Option<String>,
    },
    Command {
        program: String,
        args: Option<Vec<String>>,
    },
}

impl From<NotificationConfigFile> for NotificationConfig {
    fn from(file: NotificationConfigFile) -> Self {
        Self {
            on_waiting_input: convert_hooks(file.on_waiting_input),
            on_error: convert_hooks(file.on_error),
            on_idle_timeout: convert_hooks(file.on_idle_timeout),
            debounce_secs: file.debounce_secs.unwrap_or(300),
        }
    }
}

fn convert_hooks(hooks: Option<Vec<NotificationHookFile>>) -> Vec<NotificationHook> {
    hooks
        .unwrap_or_default()
        .into_iter()
        .map(|hook| match hook {
            NotificationHookFile::Desktop => NotificationHook::Desktop,
            NotificationHookFile::Webhook { url, template } => NotificationHook::Webhook {
                url,
                template: template
                    .unwrap_or_else(|| "Session {{session_id}} is {{state}}".to_string()),
            },
            NotificationHookFile::Command { program, args } => NotificationHook::Command {
                program,
                args: args.unwrap_or_default(),
            },
        })
        .collect()
}

pub struct SessionService<R: CommandRunner> {
    config: ForgedConfig,
    runner: R,
    store: SessionStore,
    manager: SessionManager,
    notifier: NotificationEngine,
    stream_manager: stream::StreamManager,
}

impl<R: CommandRunner> SessionService<R> {
    pub fn new(config: ForgedConfig, runner: R) -> Self {
        let store = SessionStore::new(&config.data_dir);
        let manager = SessionManager::new(store.clone());
        let ring_capacity = config.event_ring_capacity;
        let dedup_window = config.input_dedup_window;
        Self {
            config,
            runner,
            store,
            manager,
            notifier: NotificationEngine::new(),
            stream_manager: stream::StreamManager::new(ring_capacity, dedup_window),
        }
    }

    pub fn config(&self) -> &ForgedConfig {
        &self.config
    }

    pub fn stream_manager(&self) -> stream::StreamManager {
        self.stream_manager.clone()
    }

    pub fn start_session(
        &self,
        agent: AgentType,
        model: impl Into<String>,
        repo_path: impl AsRef<Path>,
    ) -> anyhow::Result<SessionRecord> {
        self.start_session_with_worktree(agent, model, repo_path, None, None)
    }

    pub fn start_session_with_worktree(
        &self,
        agent: AgentType,
        model: impl Into<String>,
        repo_path: impl AsRef<Path>,
        worktree: Option<WorktreeSpec>,
        notify: Option<Vec<NotificationKind>>,
    ) -> anyhow::Result<SessionRecord> {
        let repo_path = repo_path.as_ref();
        let repo_root = forgemux_core::RepoRoot::discover(repo_path)
            .map(|root| root.path().to_path_buf())
            .unwrap_or(repo_path.to_path_buf());

        let (session_root, worktree_info) = if let Some(spec) = worktree {
            if forgemux_core::RepoRoot::discover(repo_path).is_none() {
                anyhow::bail!("--worktree requires a git repository");
            }
            let worktree_path = spec.path.clone().unwrap_or_else(|| {
                repo_root
                    .join(".forgemux")
                    .join("worktrees")
                    .join(&spec.branch)
            });
            self.create_worktree(&repo_root, &worktree_path, &spec.branch)?;
            (worktree_path, Some(spec))
        } else {
            (repo_root, None)
        };

        let mut record = self
            .manager
            .create_session(agent.clone(), model, &session_root)
            .context("create session record")?;
        record.touch_state(SessionState::Starting);
        self.store.save(&record)?;

        let agent_cfg = self
            .config
            .agents
            .get(&agent)
            .context("missing agent config")?;

        let mut args = vec![
            "new-session".to_string(),
            "-d".to_string(),
            "-s".to_string(),
            record.id.as_str().to_string(),
            "-c".to_string(),
            session_root.to_string_lossy().to_string(),
            "--".to_string(),
            agent_cfg.command.clone(),
        ];
        args.extend(agent_cfg.args.iter().cloned());

        let output = self.runner.run(&self.config.tmux_bin, &args)?;
        if !output.status.success() {
            record.touch_state(SessionState::Errored);
            self.store.save(&record)?;
            anyhow::bail!("tmux failed: {}", String::from_utf8_lossy(&output.stderr));
        }

        self.ensure_transcript_pipe(&record)?;

        record.touch_state(SessionState::Running);
        self.store.save(&record)?;
        if let Some(spec) = worktree_info {
            self.store_worktree_meta(&record.id, &spec)?;
        }
        if let Some(kinds) = notify {
            self.store_notification_prefs(&record.id, &kinds)?;
        }
        Ok(record)
    }

    pub fn start_foreman(
        &self,
        agent: AgentType,
        model: impl Into<String>,
        repo_path: impl AsRef<Path>,
        watch_scope: Vec<String>,
        intervention: InterventionLevel,
    ) -> anyhow::Result<SessionRecord> {
        let role = SessionRole::Foreman {
            watch_scope,
            intervention,
        };
        let mut record = self
            .manager
            .create_session_with_role(agent.clone(), model, repo_path, role)
            .context("create foreman session")?;
        record.touch_state(SessionState::Starting);
        self.store.save(&record)?;

        let agent_cfg = self
            .config
            .agents
            .get(&agent)
            .context("missing agent config")?;

        let mut args = vec![
            "new-session".to_string(),
            "-d".to_string(),
            "-s".to_string(),
            record.id.as_str().to_string(),
            "--".to_string(),
            agent_cfg.command.clone(),
        ];
        args.extend(agent_cfg.args.iter().cloned());

        let output = self.runner.run(&self.config.tmux_bin, &args)?;
        if !output.status.success() {
            record.touch_state(SessionState::Errored);
            self.store.save(&record)?;
            anyhow::bail!("tmux failed: {}", String::from_utf8_lossy(&output.stderr));
        }

        self.ensure_transcript_pipe(&record)?;
        record.touch_state(SessionState::Running);
        self.store.save(&record)?;
        Ok(record)
    }

    pub fn list_sessions(&self) -> anyhow::Result<Vec<SessionRecord>> {
        let sessions = self.store.list()?;
        Ok(sort_sessions(sessions))
    }

    pub fn session(&self, id: &str) -> anyhow::Result<SessionRecord> {
        let id = forgemux_core::SessionId::from(id);
        Ok(self.store.load(&id)?)
    }

    pub fn load_session(&self, id: &forgemux_core::SessionId) -> anyhow::Result<SessionRecord> {
        Ok(self.store.load(id)?)
    }

    pub fn refresh_states(&self) -> anyhow::Result<Vec<SessionRecord>> {
        let sessions = self.store.list()?;
        let detector = self.build_detector();
        let mut updated = Vec::new();
        for mut session in sessions {
            let output = self.capture_recent_output(&session.id)?;
            let last_output_at = self.transcript_mtime(&session.id)
                .unwrap_or(session.last_activity_at);
            let signal = StateSignal {
                process_alive: self.has_session(&session.id),
                exit_code: None,
                last_output_at,
                recent_output: output,
            };
            let state = detector.detect(Utc::now(), &signal);
            if state != session.state {
                let allowed = self.load_notification_prefs(&session.id)?;
                self.notifier.maybe_notify(
                    &self.config.notifications,
                    &self.runner,
                    &session,
                    &state,
                    allowed.as_deref(),
                );
                session.touch_state(state);
                self.store.save(&session)?;
            }
            updated.push(session);
        }
        Ok(sort_sessions(updated))
    }

    pub fn stop_session(&self, id: &str) -> anyhow::Result<()> {
        let id = forgemux_core::SessionId::from(id);
        let args = vec!["kill-session".to_string(), "-t".to_string(), id.as_str().to_string()];
        let output = self.runner.run(&self.config.tmux_bin, &args)?;
        if !output.status.success() {
            anyhow::bail!("tmux kill-session failed: {}", String::from_utf8_lossy(&output.stderr));
        }
        let mut record = self.store.load(&id)?;
        record.touch_state(SessionState::Terminated);
        self.store.save(&record)?;
        Ok(())
    }

    pub fn detach_session(&self, id: &str) -> anyhow::Result<()> {
        let id = forgemux_core::SessionId::from(id);
        let args = vec![
            "detach-client".to_string(),
            "-a".to_string(),
            "-s".to_string(),
            id.as_str().to_string(),
        ];
        let output = self.runner.run(&self.config.tmux_bin, &args)?;
        if !output.status.success() {
            anyhow::bail!(
                "tmux detach-client failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }

    pub fn kill_session(&self, id: &str) -> anyhow::Result<()> {
        self.stop_session(id)
    }

    pub fn attach_session(&self, id: &str) -> anyhow::Result<()> {
        let id = forgemux_core::SessionId::from(id);
        let args = vec!["attach-session".to_string(), "-t".to_string(), id.as_str().to_string()];
        let output = self.runner.run(&self.config.tmux_bin, &args)?;
        if !output.status.success() {
            anyhow::bail!("tmux attach failed: {}", String::from_utf8_lossy(&output.stderr));
        }
        Ok(())
    }

    pub fn logs(&self, id: &str, tail: usize) -> anyhow::Result<String> {
        let id = forgemux_core::SessionId::from(id);
        let path = self.transcript_path(&id);
        if !path.exists() {
            return Ok(String::new());
        }
        let content = fs::read_to_string(path)?;
        let lines: Vec<&str> = content.lines().collect();
        let start = lines.len().saturating_sub(tail);
        Ok(lines[start..].join("\n"))
    }

    fn build_detector(&self) -> StateDetector {
        let patterns = self
            .config
            .agents
            .values()
            .flat_map(|cfg| cfg.prompt_patterns.iter())
            .filter_map(|pat| Regex::new(pat).ok())
            .collect();
        StateDetector::new(
            self.config.idle_threshold_secs,
            self.config.waiting_threshold_secs,
            patterns,
        )
    }

    fn capture_recent_output(&self, id: &forgemux_core::SessionId) -> anyhow::Result<String> {
        self.capture_output(id, 100)
    }

    pub fn capture_output(
        &self,
        id: &forgemux_core::SessionId,
        lines: i32,
    ) -> anyhow::Result<String> {
        let args = vec![
            "capture-pane".to_string(),
            "-p".to_string(),
            "-S".to_string(),
            format!("-{}", lines),
            "-t".to_string(),
            id.as_str().to_string(),
        ];
        let output = self.runner.run(&self.config.tmux_bin, &args)?;
        if !output.status.success() {
            return Ok(String::new());
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    pub fn send_keys(&self, id: &forgemux_core::SessionId, input: &str) -> anyhow::Result<()> {
        let mut literal = input.replace('\r', "");
        let mut needs_enter = false;
        if literal.contains('\n') {
            literal = literal.replace('\n', "");
            needs_enter = true;
        }

        if !literal.is_empty() {
            let args = vec![
                "send-keys".to_string(),
                "-t".to_string(),
                id.as_str().to_string(),
                "-l".to_string(),
                literal,
            ];
            let output = self.runner.run(&self.config.tmux_bin, &args)?;
            if !output.status.success() {
                anyhow::bail!(
                    "tmux send-keys failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }

        if needs_enter {
            let args = vec![
                "send-keys".to_string(),
                "-t".to_string(),
                id.as_str().to_string(),
                "Enter".to_string(),
            ];
            let output = self.runner.run(&self.config.tmux_bin, &args)?;
            if !output.status.success() {
                anyhow::bail!(
                    "tmux send-keys Enter failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
        Ok(())
    }

    fn has_session(&self, id: &forgemux_core::SessionId) -> bool {
        let args = vec!["has-session".to_string(), "-t".to_string(), id.as_str().to_string()];
        self.runner
            .run(&self.config.tmux_bin, &args)
            .map(|out| out.status.success())
            .unwrap_or(false)
    }

    fn ensure_transcript_pipe(&self, record: &SessionRecord) -> anyhow::Result<()> {
        let path = self.transcript_path(&record.id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let cmd = format!(
            "awk '{{ print strftime(\"%Y-%m-%dT%H:%M:%S%z\"), $0 }}' >> {}",
            path.display()
        );
        let args = vec![
            "pipe-pane".to_string(),
            "-o".to_string(),
            "-t".to_string(),
            record.id.as_str().to_string(),
            cmd,
        ];
        let output = self.runner.run(&self.config.tmux_bin, &args)?;
        if !output.status.success() {
            anyhow::bail!("tmux pipe-pane failed: {}", String::from_utf8_lossy(&output.stderr));
        }
        Ok(())
    }

    fn create_worktree(
        &self,
        repo_root: &Path,
        worktree_path: &Path,
        branch: &str,
    ) -> anyhow::Result<()> {
        if worktree_path.exists() {
            anyhow::bail!("worktree path already exists: {}", worktree_path.display());
        }
        if let Some(parent) = worktree_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let args = vec![
            "-C".to_string(),
            repo_root.to_string_lossy().to_string(),
            "worktree".to_string(),
            "add".to_string(),
            "-b".to_string(),
            branch.to_string(),
            worktree_path.to_string_lossy().to_string(),
        ];
        let output = self.runner.run("git", &args)?;
        if !output.status.success() {
            anyhow::bail!(
                "git worktree add failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }

    fn store_worktree_meta(
        &self,
        id: &forgemux_core::SessionId,
        spec: &WorktreeSpec,
    ) -> anyhow::Result<()> {
        let path = self
            .config
            .data_dir
            .join("worktrees")
            .join(format!("{}.json", id.as_str()));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_vec_pretty(spec)?;
        fs::write(path, data)?;
        Ok(())
    }

    fn transcript_path(&self, id: &forgemux_core::SessionId) -> PathBuf {
        self.config.data_dir.join("transcripts").join(format!("{}.log", id.as_str()))
    }

    fn transcript_mtime(&self, id: &forgemux_core::SessionId) -> Option<DateTime<Utc>> {
        let path = self.transcript_path(id);
        let meta = fs::metadata(path).ok()?;
        let modified = meta.modified().ok()?;
        Some(system_time_to_chrono(modified))
    }

    fn notification_prefs_path(&self, id: &forgemux_core::SessionId) -> PathBuf {
        self.config
            .data_dir
            .join("notifications")
            .join(format!("{}.json", id.as_str()))
    }

    fn store_notification_prefs(
        &self,
        id: &forgemux_core::SessionId,
        kinds: &[NotificationKind],
    ) -> anyhow::Result<()> {
        let path = self.notification_prefs_path(id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let prefs = NotificationPrefs {
            kinds: kinds.to_vec(),
        };
        let data = serde_json::to_vec_pretty(&prefs)?;
        fs::write(path, data)?;
        Ok(())
    }

    fn load_notification_prefs(
        &self,
        id: &forgemux_core::SessionId,
    ) -> anyhow::Result<Option<Vec<NotificationKind>>> {
        let path = self.notification_prefs_path(id);
        if !path.exists() {
            return Ok(None);
        }
        let data = fs::read(path)?;
        let prefs: NotificationPrefs = serde_json::from_slice(&data)?;
        Ok(Some(prefs.kinds))
    }
}

fn system_time_to_chrono(time: SystemTime) -> DateTime<Utc> {
    DateTime::<Utc>::from(time)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorktreeSpec {
    pub branch: String,
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct NotificationPrefs {
    kinds: Vec<NotificationKind>,
}

#[cfg(test)]
impl Default for WorktreeSpec {
    fn default() -> Self {
        Self {
            branch: "test-branch".to_string(),
            path: None,
        }
    }
}

#[derive(Default)]
pub struct NotificationEngine {
    last_fired: std::sync::Mutex<HashMap<(String, NotificationEvent), DateTime<Utc>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotificationEvent {
    WaitingInput,
    Errored,
    IdleTimeout,
}

impl NotificationEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn maybe_notify<R: CommandRunner>(
        &self,
        config: &NotificationConfig,
        runner: &R,
        session: &SessionRecord,
        new_state: &SessionState,
        allowed: Option<&[NotificationKind]>,
    ) {
        let event = match new_state {
            SessionState::WaitingInput => Some(NotificationEvent::WaitingInput),
            SessionState::Errored => Some(NotificationEvent::Errored),
            SessionState::Terminated => Some(NotificationEvent::IdleTimeout),
            _ => None,
        };
        let Some(event) = event else { return; };

        if !self.should_fire(session, event, config.debounce_secs) {
            return;
        }

        let hooks = match event {
            NotificationEvent::WaitingInput => &config.on_waiting_input,
            NotificationEvent::Errored => &config.on_error,
            NotificationEvent::IdleTimeout => &config.on_idle_timeout,
        };

        for hook in hooks {
            if let Some(kinds) = allowed {
                if !kinds.contains(&hook.kind()) {
                    continue;
                }
            }
            let _ = send_hook(runner, hook, session, event);
        }
    }

    fn should_fire(&self, session: &SessionRecord, event: NotificationEvent, debounce: i64) -> bool {
        let now = Utc::now();
        let key = (session.id.as_str().to_string(), event);
        let mut guard = self.last_fired.lock().unwrap();
        if let Some(last) = guard.get(&key) {
            if (now - *last).num_seconds() < debounce {
                return false;
            }
        }
        guard.insert(key, now);
        true
    }
}

impl NotificationHook {
    fn kind(&self) -> NotificationKind {
        match self {
            NotificationHook::Desktop => NotificationKind::Desktop,
            NotificationHook::Webhook { .. } => NotificationKind::Webhook,
            NotificationHook::Command { .. } => NotificationKind::Command,
        }
    }
}
fn send_hook<R: CommandRunner>(
    runner: &R,
    hook: &NotificationHook,
    session: &SessionRecord,
    event: NotificationEvent,
) -> anyhow::Result<()> {
    let message = render_template(
        "Session {{session_id}} is {{state}}",
        session,
        event,
    );
    match hook {
        NotificationHook::Desktop => {
            let args = vec!["-a".to_string(), "forgemux".to_string(), message];
            let _ = runner.run("notify-send", &args)?;
        }
        NotificationHook::Command { program, args } => {
            let rendered = args
                .iter()
                .map(|arg| render_template(arg, session, event))
                .collect::<Vec<_>>();
            let _ = runner.run(program, &rendered)?;
        }
        NotificationHook::Webhook { url, template } => {
            let body = render_template(template, session, event);
            let client = reqwest::blocking::Client::new();
            let resp = client.post(url).body(body).send()?;
            if !resp.status().is_success() {
                anyhow::bail!("webhook returned {}", resp.status());
            }
        }
    }
    Ok(())
}

fn render_template(template: &str, session: &SessionRecord, event: NotificationEvent) -> String {
    let state = match event {
        NotificationEvent::WaitingInput => "waiting",
        NotificationEvent::Errored => "errored",
        NotificationEvent::IdleTimeout => "terminated",
    };
    template
        .replace("{{session_id}}", session.id.as_str())
        .replace("{{state}}", state)
        .replace("{{agent}}", &format!("{:?}", session.agent))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_session_invokes_tmux_new_session() {
        let tmp = tempfile::tempdir().unwrap();
        let mut config = ForgedConfig::default_with_data_dir(tmp.path().to_path_buf());
        config.tmux_bin = "tmux".to_string();
        let runner = FakeRunner::default();
        let service = SessionService::new(config, runner.clone());

        let record = service
            .start_session(AgentType::Claude, "sonnet", tmp.path())
            .unwrap();

        let calls = runner.calls();
        assert!(calls.iter().any(|call| {
            call.contains(&"new-session".to_string())
                && call.contains(&record.id.as_str().to_string())
        }));
    }

    #[test]
    fn start_session_records_error_on_tmux_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let config = ForgedConfig::default_with_data_dir(tmp.path().to_path_buf());
        let runner = FakeRunner {
            should_fail: true,
            ..Default::default()
        };
        let service = SessionService::new(config, runner);

        let result = service.start_session(AgentType::Claude, "sonnet", tmp.path());
        assert!(result.is_err());

        let sessions = service.list_sessions().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].state, SessionState::Errored);
    }

    #[test]
    fn notification_engine_debounces() {
        let engine = NotificationEngine::new();
        let mut record = SessionRecord::new(
            AgentType::Claude,
            "sonnet",
            PathBuf::from("/tmp"),
        );
        record.id = forgemux_core::SessionId::from("S-0001");

        let config = NotificationConfig {
            on_waiting_input: vec![NotificationHook::Desktop],
            on_error: vec![],
            on_idle_timeout: vec![],
            debounce_secs: 60,
        };

        let runner = FakeRunner::default();
        engine.maybe_notify(
            &config,
            &runner,
            &record,
            &SessionState::WaitingInput,
            None,
        );
        engine.maybe_notify(
            &config,
            &runner,
            &record,
            &SessionState::WaitingInput,
            None,
        );

        let calls = runner.calls();
        assert_eq!(calls.len(), 1);
    }

    #[test]
    fn render_template_expands_session_values() {
        let record = SessionRecord::new(
            AgentType::Codex,
            "o3",
            PathBuf::from("/tmp"),
        );
        let rendered = render_template(
            "id={{session_id}} state={{state}} agent={{agent}}",
            &record,
            NotificationEvent::Errored,
        );
        assert!(rendered.contains(record.id.as_str()));
        assert!(rendered.contains("errored"));
        assert!(rendered.contains("Codex"));
    }

    #[test]
    fn notification_kinds_filter_hooks() {
        let engine = NotificationEngine::new();
        let mut record = SessionRecord::new(
            AgentType::Claude,
            "sonnet",
            PathBuf::from("/tmp"),
        );
        record.id = forgemux_core::SessionId::from("S-0002");

        let config = NotificationConfig {
            on_waiting_input: vec![
                NotificationHook::Desktop,
                NotificationHook::Command {
                    program: "echo".to_string(),
                    args: vec!["hi".to_string()],
                },
            ],
            on_error: vec![],
            on_idle_timeout: vec![],
            debounce_secs: 0,
        };

        let runner = FakeRunner::default();
        engine.maybe_notify(
            &config,
            &runner,
            &record,
            &SessionState::WaitingInput,
            Some(&[NotificationKind::Command]),
        );

        let calls = runner.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0][0], "echo");
    }

    #[test]
    fn create_worktree_runs_git() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let status = Command::new("git").arg("init").arg(&repo).status().unwrap();
        assert!(status.success());

        let config = ForgedConfig::default_with_data_dir(tmp.path().to_path_buf());
        let runner = FakeRunner::default();
        let service = SessionService::new(config, runner.clone());

        let worktree_path = tmp.path().join("wt");
        service
            .create_worktree(&repo, &worktree_path, "test-branch")
            .unwrap();

        let calls = runner.calls();
        assert!(calls.iter().any(|call| {
            call.contains(&"git".to_string())
                && call.contains(&"worktree".to_string())
                && call.contains(&"add".to_string())
        }));
    }

    #[test]
    fn send_keys_uses_tmux() {
        let tmp = tempfile::tempdir().unwrap();
        let config = ForgedConfig::default_with_data_dir(tmp.path().to_path_buf());
        let runner = FakeRunner::default();
        let service = SessionService::new(config, runner.clone());

        let id = forgemux_core::SessionId::from("S-TEST");
        service.send_keys(&id, "echo hi\n").unwrap();

        let calls = runner.calls();
        assert!(calls.iter().any(|call| call.contains(&"send-keys".to_string())));
    }

    #[test]
    fn start_session_persists_notification_prefs() {
        let tmp = tempfile::tempdir().unwrap();
        let config = ForgedConfig::default_with_data_dir(tmp.path().to_path_buf());
        let runner = FakeRunner::default();
        let service = SessionService::new(config, runner);

        let record = service
            .start_session_with_worktree(
                AgentType::Claude,
                "sonnet",
                tmp.path(),
                None,
                Some(vec![NotificationKind::Desktop]),
            )
            .unwrap();

        let prefs_path = tmp
            .path()
            .join("notifications")
            .join(format!("{}.json", record.id.as_str()));
        let data = std::fs::read_to_string(prefs_path).unwrap();
        assert!(data.contains("Desktop"));
    }

    #[test]
    fn load_config_overrides_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join("forged.toml");
        let contents = r#"
data_dir = "/tmp/forgemux-data"
tmux_bin = "tmux-custom"
idle_threshold_secs = 120
waiting_threshold_secs = 25
node_id = "edge-01"
hub_url = "http://hub.local:8080"
advertise_addr = "edge-01.local:9443"
event_ring_capacity = 42
input_dedup_window = 55
snapshot_lines = 123
poll_interval_ms = 10

[agents.claude]
command = "claude-custom"
args = ["--model", "haiku"]
prompt_patterns = ["(?m)^>$"]

[notifications]
debounce_secs = 42

[[notifications.on_waiting_input]]
kind = "desktop"

[[notifications.on_error]]
kind = "command"
program = "echo"
args = ["session={{session_id}}"]
"#;
        std::fs::write(&config_path, contents).unwrap();

        let config = ForgedConfig::load(&config_path).unwrap();
        assert_eq!(config.data_dir, PathBuf::from("/tmp/forgemux-data"));
        assert_eq!(config.tmux_bin, "tmux-custom");
        assert_eq!(config.idle_threshold_secs, 120);
        assert_eq!(config.waiting_threshold_secs, 25);
        assert_eq!(config.node_id.as_deref(), Some("edge-01"));
        assert_eq!(config.hub_url.as_deref(), Some("http://hub.local:8080"));
        assert_eq!(
            config.advertise_addr.as_deref(),
            Some("edge-01.local:9443")
        );
        assert_eq!(config.event_ring_capacity, 42);
        assert_eq!(config.input_dedup_window, 55);
        assert_eq!(config.snapshot_lines, 123);
        assert_eq!(config.poll_interval_ms, 10);
        let claude = config.agents.get(&AgentType::Claude).unwrap();
        assert_eq!(claude.command, "claude-custom");
        assert_eq!(claude.args, vec!["--model".to_string(), "haiku".to_string()]);
        assert_eq!(claude.prompt_patterns, vec!["(?m)^>$".to_string()]);
        assert_eq!(config.notifications.debounce_secs, 42);
        assert_eq!(config.notifications.on_waiting_input.len(), 1);
        assert_eq!(config.notifications.on_error.len(), 1);
    }

    #[test]
    fn load_config_rejects_unknown_agent() {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join("forged.toml");
        let contents = r#"
[agents.unknown]
command = "bad"
"#;
        std::fs::write(&config_path, contents).unwrap();
        let err = ForgedConfig::load(&config_path).unwrap_err();
        assert!(err.to_string().contains("unknown agent"));
    }
}
