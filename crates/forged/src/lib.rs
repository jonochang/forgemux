use anyhow::Context;
use chrono::{DateTime, Utc};
use forgemux_core::{
    AgentType, InterventionLevel, ReplayEvent, ReplayEventType, SessionManager, SessionRecord,
    SessionRole, SessionState, SessionStore, StateDetector, StateSignal, sort_sessions,
};
use regex::Regex;
use std::collections::VecDeque;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::fs::OpenOptions;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::SystemTime;

mod agent;
pub mod checks;
pub mod server;
pub mod stream;

use agent::{AgentAdapter, AgentLogSignal, ConfigAdapter};

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
#[allow(clippy::type_complexity)]
pub struct FakeRunner {
    calls: std::sync::Arc<std::sync::Mutex<Vec<Vec<String>>>>,
    pub should_fail: bool,
    overrides: std::sync::Arc<std::sync::Mutex<Vec<(Vec<String>, i32)>>>,
    stdout_overrides: std::sync::Arc<std::sync::Mutex<Vec<(Vec<String>, Vec<u8>)>>>,
}

#[cfg(test)]
impl FakeRunner {
    pub fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().unwrap().clone()
    }

    pub fn set_status_for(&self, pattern: &[&str], status_code: i32) {
        let mut overrides = self.overrides.lock().unwrap();
        overrides.push((pattern.iter().map(|s| s.to_string()).collect(), status_code));
    }

    pub fn set_stdout_for(&self, pattern: &[&str], stdout: &[u8]) {
        let mut overrides = self.stdout_overrides.lock().unwrap();
        overrides.push((
            pattern.iter().map(|s| s.to_string()).collect(),
            stdout.to_vec(),
        ));
    }
}

#[cfg(test)]
impl CommandRunner for FakeRunner {
    fn run(&self, program: &str, args: &[String]) -> std::io::Result<Output> {
        use std::os::unix::process::ExitStatusExt;
        let mut call = vec![program.to_string()];
        call.extend_from_slice(args);
        self.calls.lock().unwrap().push(call);
        let status_code = if self.should_fail {
            1
        } else {
            let overrides = self.overrides.lock().unwrap();
            let mut code = 0;
            for (pattern, status_code) in overrides.iter() {
                if pattern
                    .iter()
                    .all(|token| args.contains(token) || program == token)
                {
                    code = *status_code;
                    break;
                }
            }
            code
        };
        let status = std::process::ExitStatus::from_raw(status_code);
        let stdout = {
            let overrides = self.stdout_overrides.lock().unwrap();
            let mut data = Vec::new();
            for (pattern, payload) in overrides.iter() {
                if pattern
                    .iter()
                    .all(|token| args.contains(token) || program == token)
                {
                    data = payload.clone();
                    break;
                }
            }
            data
        };
        Ok(Output {
            status,
            stdout,
            stderr: Vec::new(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub command: String,
    pub args: Vec<String>,
    pub prompt_patterns: Vec<String>,
    pub usage_paths: Vec<String>,
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
    pub rate_limit_per_hour: u32,
}

#[derive(Clone)]
struct AgentAdapters {
    adapters: HashMap<AgentType, ConfigAdapter>,
}

impl AgentAdapters {
    fn new(config: &ForgedConfig) -> Self {
        let mut adapters = HashMap::new();
        for (agent, agent_config) in &config.agents {
            let prompt_patterns = agent_config
                .prompt_patterns
                .iter()
                .filter_map(|pat| Regex::new(pat).ok())
                .collect::<Vec<_>>();
            let usage_paths = agent_config
                .usage_paths
                .iter()
                .map(|path| expand_tilde(path))
                .collect::<Vec<_>>();
            adapters.insert(
                agent.clone(),
                ConfigAdapter::new(agent.clone(), prompt_patterns, usage_paths),
            );
        }
        Self { adapters }
    }

    fn adapter_for(&self, agent: &AgentType) -> Option<&ConfigAdapter> {
        self.adapters.get(agent)
    }

    fn prompt_patterns(&self) -> Vec<Regex> {
        self.adapters
            .values()
            .flat_map(|adapter| adapter.prompt_patterns().to_vec())
            .collect()
    }
}

#[derive(Default)]
struct LogWatcher {
    cursors: HashMap<forgemux_core::SessionId, LogCursor>,
}

#[derive(Clone)]
struct LogCursor {
    path: Option<PathBuf>,
    offset: u64,
    waiting_since: Option<DateTime<Utc>>,
}

#[derive(Debug)]
pub struct PidLock {
    path: PathBuf,
}

impl Drop for PidLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
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
    pub hub_token: Option<String>,
    pub advertise_addr: Option<String>,
    pub event_ring_capacity: usize,
    pub input_dedup_window: usize,
    pub snapshot_lines: i32,
    pub poll_interval_ms: u64,
    pub snapshot_interval_ms: u64,
    pub stream_encryption_key: Option<String>,
    pub policies: HashMap<String, PolicyConfig>,
    pub api_tokens: Vec<String>,
    pub default_repo: Option<PathBuf>,
}

impl ForgedConfig {
    pub fn default_with_data_dir(data_dir: PathBuf) -> Self {
        let mut agents = HashMap::new();
        agents.insert(
            AgentType::Claude,
            AgentConfig {
                command: "claude".to_string(),
                args: vec!["--dangerously-skip-permissions".to_string()],
                prompt_patterns: vec![r"(?m)^>\s*$".to_string()],
                usage_paths: vec![
                    "~/.config/claude/projects/".to_string(),
                    "~/.claude/projects/".to_string(),
                ],
            },
        );
        agents.insert(
            AgentType::Codex,
            AgentConfig {
                command: "codex".to_string(),
                args: vec![],
                prompt_patterns: vec![r"(?m)^(?:> |\$ )".to_string()],
                usage_paths: vec!["~/.codex/sessions/".to_string()],
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
                rate_limit_per_hour: 60,
            },
            node_id: None,
            hub_url: None,
            hub_token: None,
            advertise_addr: None,
            event_ring_capacity: 512,
            input_dedup_window: 1000,
            snapshot_lines: 5000,
            poll_interval_ms: 250,
            snapshot_interval_ms: 30_000,
            stream_encryption_key: None,
            policies: HashMap::new(),
            api_tokens: Vec::new(),
            default_repo: None,
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
                if let Some(usage_paths) = agent.usage_paths {
                    entry.usage_paths = usage_paths;
                }
            }
        }
        if let Some(notifications) = file.notifications {
            config.notifications = notifications.into();
        }
        config.node_id = file.node_id;
        config.hub_url = file.hub_url;
        config.hub_token = file.hub_token;
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
        if let Some(snapshot_interval_ms) = file.snapshot_interval_ms {
            config.snapshot_interval_ms = snapshot_interval_ms;
        }
        config.stream_encryption_key = file.stream_encryption_key;
        if let Some(policies) = file.policies {
            config.policies = policies;
        }
        if let Some(api_tokens) = file.api_tokens {
            config.api_tokens = api_tokens;
        }
        config.default_repo = file.default_repo;
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
    pub hub_token: Option<String>,
    pub advertise_addr: Option<String>,
    pub event_ring_capacity: Option<usize>,
    pub input_dedup_window: Option<usize>,
    pub snapshot_lines: Option<i32>,
    pub poll_interval_ms: Option<u64>,
    pub snapshot_interval_ms: Option<u64>,
    pub stream_encryption_key: Option<String>,
    pub policies: Option<HashMap<String, PolicyConfig>>,
    pub api_tokens: Option<Vec<String>>,
    pub default_repo: Option<PathBuf>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PolicyConfig {
    pub cpu_shares: Option<u64>,
    pub memory_max: Option<String>,
    pub pids_max: Option<u64>,
    pub network: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct AgentFile {
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub prompt_patterns: Option<Vec<String>>,
    pub usage_paths: Option<Vec<String>>,
}

#[derive(Debug, serde::Deserialize)]
struct NotificationConfigFile {
    pub on_waiting_input: Option<Vec<NotificationHookFile>>,
    pub on_error: Option<Vec<NotificationHookFile>>,
    pub on_idle_timeout: Option<Vec<NotificationHookFile>>,
    pub debounce_secs: Option<i64>,
    pub rate_limit_per_hour: Option<u32>,
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
            rate_limit_per_hour: file.rate_limit_per_hour.unwrap_or(60),
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
    adapters: AgentAdapters,
    log_watcher: std::sync::Mutex<LogWatcher>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ForemanReport {
    pub generated_at: DateTime<Utc>,
    pub sessions: Vec<ForemanSessionSummary>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ForemanSessionSummary {
    pub id: String,
    pub agent: AgentType,
    pub model: String,
    pub state: SessionState,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UsageRecord {
    pub session_id: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub estimated_cost_usd: f64,
}

impl<R: CommandRunner> SessionService<R> {
    pub fn new(config: ForgedConfig, runner: R) -> Self {
        let store = SessionStore::new(&config.data_dir);
        let manager = SessionManager::new(store.clone());
        let ring_capacity = config.event_ring_capacity;
        let dedup_window = config.input_dedup_window;
        let adapters = AgentAdapters::new(&config);
        Self {
            config,
            runner,
            store,
            manager,
            notifier: NotificationEngine::new(),
            stream_manager: stream::StreamManager::new(ring_capacity, dedup_window),
            adapters,
            log_watcher: std::sync::Mutex::new(LogWatcher::default()),
        }
    }

    pub fn config(&self) -> &ForgedConfig {
        &self.config
    }

    pub fn stream_manager(&self) -> stream::StreamManager {
        self.stream_manager.clone()
    }

    pub fn acquire_pid_lock(&self) -> anyhow::Result<PidLock> {
        let path = self.config.data_dir.join("forged.pid");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        if path.exists() {
            let contents = fs::read_to_string(&path).unwrap_or_default();
            if let Ok(pid) = contents.trim().parse::<u32>()
                && pid_is_alive(pid)
            {
                anyhow::bail!("forged already running (pid {})", pid);
            }
        }
        fs::write(&path, std::process::id().to_string())?;
        Ok(PidLock { path })
    }

    pub fn cleanup_orphan_sessions(&self) -> anyhow::Result<usize> {
        let sessions_dir = self.config.data_dir.join("sessions");
        if !sessions_dir.exists() {
            return Ok(0);
        }
        let args = vec![
            "list-sessions".to_string(),
            "-F".to_string(),
            "#{session_name}".to_string(),
        ];
        let output = self.runner.run(&self.config.tmux_bin, &args)?;
        if !output.status.success() {
            return Ok(0);
        }
        let known = self
            .store
            .list()?
            .into_iter()
            .map(|s| s.id)
            .collect::<HashSet<_>>();
        let mut reaped = 0;
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let name = line.trim();
            if !name.starts_with("S-") {
                continue;
            }
            let id = forgemux_core::SessionId::from(name);
            if known.contains(&id) {
                continue;
            }
            let args = vec![
                "kill-session".to_string(),
                "-t".to_string(),
                name.to_string(),
            ];
            let _ = self.runner.run(&self.config.tmux_bin, &args)?;
            reaped += 1;
        }
        Ok(reaped)
    }

    pub fn start_session(
        &self,
        agent: AgentType,
        model: impl Into<String>,
        repo_path: impl AsRef<Path>,
    ) -> anyhow::Result<SessionRecord> {
        self.start_session_with_worktree(agent, model, repo_path, None, None, None)
    }

    pub fn start_session_with_worktree(
        &self,
        agent: AgentType,
        model: impl Into<String>,
        repo_path: impl AsRef<Path>,
        worktree: Option<WorktreeSpec>,
        notify: Option<Vec<NotificationKind>>,
        policy: Option<String>,
    ) -> anyhow::Result<SessionRecord> {
        if self.is_draining() {
            anyhow::bail!("forged is draining; no new sessions allowed");
        }
        let mut repo_path = repo_path.as_ref().to_path_buf();
        if repo_path.as_os_str().is_empty() {
            if let Some(default_repo) = &self.config.default_repo {
                repo_path = default_repo.clone();
            } else {
                anyhow::bail!("repo is required (set default_repo in forged config)");
            }
        }
        let repo_root = forgemux_core::RepoRoot::discover(&repo_path)
            .map(|root| root.path().to_path_buf())
            .unwrap_or(repo_path.clone());

        let (session_root, worktree_info) = if let Some(spec) = worktree {
            if forgemux_core::RepoRoot::discover(&repo_path).is_none() {
                anyhow::bail!("--worktree requires a git repository");
            }
            let worktree_path = spec
                .path
                .clone()
                .unwrap_or_else(|| self.config.data_dir.join("worktrees").join(&spec.branch));
            self.create_worktree(&repo_root, &worktree_path, &spec.branch)?;
            (worktree_path, Some(spec))
        } else {
            (repo_root, None)
        };

        let mut record = self
            .manager
            .create_session(agent.clone(), model, &session_root)
            .context("create session record")?;
        if let Some(policy_name) = policy.clone() {
            if !self.config.policies.contains_key(&policy_name) {
                anyhow::bail!("unknown policy: {}", policy_name);
            }
            record.policy = Some(policy_name);
        }
        let expected = record.version;
        record.touch_state(SessionState::Starting);
        self.store.save_checked(&record, expected)?;
        self.log_state_change(
            &record.id,
            SessionState::Provisioning,
            SessionState::Starting,
        );

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
            let expected = record.version;
            record.touch_state(SessionState::Errored);
            self.store.save_checked(&record, expected)?;
            self.log_state_change(&record.id, SessionState::Starting, SessionState::Errored);
            anyhow::bail!("tmux failed: {}", String::from_utf8_lossy(&output.stderr));
        }

        self.ensure_transcript_pipe(&record)?;
        self.set_tmux_env(&record.id, "SESSION_ID", record.id.as_str())?;
        if let Some(addr) = self.config.advertise_addr.clone() {
            self.set_tmux_env(&record.id, "FORGED_ADDR", &addr)?;
            if let Some(port) = addr.rsplit(':').next() {
                self.set_tmux_env(&record.id, "FORGED_PORT", port)?;
            }
        }

        let expected = record.version;
        record.touch_state(SessionState::Running);
        self.store.save_checked(&record, expected)?;
        self.log_state_change(&record.id, SessionState::Starting, SessionState::Running);
        if let Some(spec) = worktree_info {
            self.store_worktree_meta(&record.id, &spec)?;
        }
        if let Some(kinds) = notify {
            self.store_notification_prefs(&record.id, &kinds)?;
        }
        self.record_replay_event(
            &record.id,
            ReplayEventType::System,
            "Session started",
            None,
            None,
        );
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
        let expected = record.version;
        record.touch_state(SessionState::Starting);
        self.store.save_checked(&record, expected)?;
        self.log_state_change(
            &record.id,
            SessionState::Provisioning,
            SessionState::Starting,
        );

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
            let expected = record.version;
            record.touch_state(SessionState::Errored);
            self.store.save_checked(&record, expected)?;
            self.log_state_change(&record.id, SessionState::Starting, SessionState::Errored);
            anyhow::bail!("tmux failed: {}", String::from_utf8_lossy(&output.stderr));
        }

        self.ensure_transcript_pipe(&record)?;
        self.set_tmux_env(&record.id, "SESSION_ID", record.id.as_str())?;
        if let Some(addr) = self.config.advertise_addr.clone() {
            self.set_tmux_env(&record.id, "FORGED_ADDR", &addr)?;
            if let Some(port) = addr.rsplit(':').next() {
                self.set_tmux_env(&record.id, "FORGED_PORT", port)?;
            }
        }
        let expected = record.version;
        record.touch_state(SessionState::Running);
        self.store.save_checked(&record, expected)?;
        self.log_state_change(&record.id, SessionState::Starting, SessionState::Running);
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
            let last_output_at = self
                .transcript_mtime(&session.id)
                .unwrap_or(session.last_activity_at);
            let waiting_hint = self.waiting_hint(&session, last_output_at);
            let signal = StateSignal {
                process_alive: self.has_session(&session.id),
                exit_code: None,
                last_output_at,
                recent_output: output,
                waiting_hint,
            };
            let state = detector.detect(Utc::now(), &signal);
            if state != session.state {
                self.log_state_change(&session.id, session.state.clone(), state.clone());
                let allowed = self.load_notification_prefs(&session.id)?;
                self.notifier.maybe_notify(
                    &self.config.notifications,
                    &self.runner,
                    &self.config.data_dir,
                    &session,
                    &state,
                    allowed.as_deref(),
                );
                let expected = session.version;
                session.touch_state(state);
                self.store.save_checked(&session, expected)?;
            }
            updated.push(session);
        }
        Ok(sort_sessions(updated))
    }

    pub fn stop_session(&self, id: &str) -> anyhow::Result<()> {
        let id = forgemux_core::SessionId::from(id);
        let args = vec![
            "kill-session".to_string(),
            "-t".to_string(),
            id.as_str().to_string(),
        ];
        let output = self.runner.run(&self.config.tmux_bin, &args)?;
        if !output.status.success() {
            anyhow::bail!(
                "tmux kill-session failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let mut record = self.store.load(&id)?;
        let from_state = record.state.clone();
        let expected = record.version;
        record.touch_state(SessionState::Terminated);
        self.store.save_checked(&record, expected)?;
        self.log_state_change(&record.id, from_state, SessionState::Terminated);
        Ok(())
    }

    pub fn drain(&self, force: bool) -> anyhow::Result<()> {
        let path = self.drain_marker();
        fs::write(path, b"draining")?;
        if force {
            let sessions = self.list_sessions()?;
            for session in sessions {
                let _ = self.stop_session(session.id.as_str());
            }
        }
        Ok(())
    }

    pub fn is_draining(&self) -> bool {
        self.drain_marker().exists()
    }

    fn drain_marker(&self) -> PathBuf {
        self.config.data_dir.join("drain")
    }

    pub fn foreman_report(&self) -> anyhow::Result<ForemanReport> {
        let sessions = self.list_sessions()?;
        let summaries = sessions
            .into_iter()
            .map(|session| ForemanSessionSummary {
                id: session.id.as_str().to_string(),
                agent: session.agent,
                model: session.model,
                state: session.state,
            })
            .collect();
        Ok(ForemanReport {
            generated_at: Utc::now(),
            sessions: summaries,
        })
    }

    pub fn usage(&self, id: &str) -> anyhow::Result<UsageRecord> {
        let id = forgemux_core::SessionId::from(id);
        let path = self.usage_path(&id);
        if !path.exists() {
            return Ok(UsageRecord {
                session_id: id.as_str().to_string(),
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
                estimated_cost_usd: 0.0,
            });
        }
        let data = fs::read(path)?;
        let record: UsageRecord = serde_json::from_slice(&data)?;
        Ok(record)
    }

    fn usage_path(&self, id: &forgemux_core::SessionId) -> PathBuf {
        self.config
            .data_dir
            .join("usage")
            .join(format!("{}.json", id.as_str()))
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
        let status = Command::new(&self.config.tmux_bin)
            .arg("attach-session")
            .arg("-t")
            .arg(id.as_str())
            .status()?;
        if !status.success() {
            anyhow::bail!("tmux attach failed");
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

    pub fn replay_jsonl(&self, id: &str) -> anyhow::Result<String> {
        let id = forgemux_core::SessionId::from(id);
        let path = self.replay_path(&id);
        if !path.exists() {
            return Ok(String::new());
        }
        Ok(fs::read_to_string(path)?)
    }

    fn build_detector(&self) -> StateDetector {
        let patterns = self.adapters.prompt_patterns();
        StateDetector::new(
            self.config.idle_threshold_secs,
            self.config.waiting_threshold_secs,
            patterns,
        )
    }

    fn waiting_hint(&self, session: &SessionRecord, last_output_at: DateTime<Utc>) -> bool {
        let Some(adapter) = self.adapters.adapter_for(&session.agent) else {
            return false;
        };
        let mut watcher = self.log_watcher.lock().unwrap();
        let cursor = watcher
            .cursors
            .entry(session.id.clone())
            .or_insert_with(|| LogCursor {
                path: None,
                offset: 0,
                waiting_since: None,
            });

        if cursor.path.is_none() {
            cursor.path = self.resolve_log_path(adapter, &session.repo_root);
            cursor.offset = 0;
        }

        if let Some(path) = cursor.path.clone() {
            self.read_log_updates(adapter, &path, cursor);
        }

        if let Some(waiting_since) = cursor.waiting_since
            && last_output_at > waiting_since
        {
            cursor.waiting_since = None;
        }

        cursor.waiting_since.is_some()
    }

    fn resolve_log_path(&self, adapter: &dyn AgentAdapter, repo_root: &Path) -> Option<PathBuf> {
        for path in adapter.log_paths(repo_root) {
            if path.is_file() && is_jsonl(&path) {
                return Some(path);
            }
            if path.is_dir()
                && let Some(found) = find_latest_jsonl(&path)
            {
                return Some(found);
            }
        }
        None
    }

    fn read_log_updates(&self, adapter: &dyn AgentAdapter, path: &Path, cursor: &mut LogCursor) {
        let Ok(metadata) = fs::metadata(path) else {
            return;
        };
        if metadata.len() < cursor.offset {
            cursor.offset = 0;
        }
        let mut file = match fs::File::open(path) {
            Ok(file) => file,
            Err(_) => return,
        };
        if file.seek(std::io::SeekFrom::Start(cursor.offset)).is_err() {
            return;
        }
        let mut buf = String::new();
        if file.read_to_string(&mut buf).is_err() {
            return;
        }
        cursor.offset = cursor.offset.saturating_add(buf.len() as u64);
        for line in buf.lines() {
            if let Some(AgentLogSignal::WaitingInput) = adapter.parse_log_line(line) {
                cursor.waiting_since = Some(Utc::now());
            }
        }
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
        let cleaned = input.replace('\r', "");
        let needs_enter = cleaned.ends_with('\n');
        let literal = cleaned.trim_end_matches('\n');

        if !literal.is_empty() {
            let args = vec![
                "send-keys".to_string(),
                "-t".to_string(),
                id.as_str().to_string(),
                "-l".to_string(),
                literal.to_string(),
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
        self.log_event(
            id,
            serde_json::json!({
                "ts": Utc::now(),
                "type": "user-input",
                "session_id": id.as_str(),
                "bytes": input.len(),
            }),
        );
        Ok(())
    }

    fn set_tmux_env(
        &self,
        id: &forgemux_core::SessionId,
        key: &str,
        value: &str,
    ) -> anyhow::Result<()> {
        let args = vec![
            "set-environment".to_string(),
            "-t".to_string(),
            id.as_str().to_string(),
            key.to_string(),
            value.to_string(),
        ];
        let output = self.runner.run(&self.config.tmux_bin, &args)?;
        if !output.status.success() {
            anyhow::bail!(
                "tmux set-environment failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }

    fn has_session(&self, id: &forgemux_core::SessionId) -> bool {
        let args = vec![
            "has-session".to_string(),
            "-t".to_string(),
            id.as_str().to_string(),
        ];
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
            anyhow::bail!(
                "tmux pipe-pane failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
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
            return Ok(());
        }
        if let Some(parent) = worktree_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let fetch_args = vec![
            "-C".to_string(),
            repo_root.to_string_lossy().to_string(),
            "fetch".to_string(),
            "--prune".to_string(),
        ];
        let fetch_output = self.runner.run("git", &fetch_args)?;
        if !fetch_output.status.success() {
            anyhow::bail!(
                "git fetch failed: {}",
                String::from_utf8_lossy(&fetch_output.stderr)
            );
        }
        let branch_exists = self.branch_exists(repo_root, branch).unwrap_or(false);
        let remote_exists = if branch_exists {
            false
        } else {
            self.remote_branch_exists(repo_root, branch)
                .unwrap_or(false)
        };
        let mut args = vec![
            "-C".to_string(),
            repo_root.to_string_lossy().to_string(),
            "worktree".to_string(),
            "add".to_string(),
        ];
        if !branch_exists {
            args.push("-b".to_string());
            args.push(branch.to_string());
        }
        args.push(worktree_path.to_string_lossy().to_string());
        if branch_exists {
            args.push(branch.to_string());
        } else if remote_exists {
            args.push(format!("origin/{}", branch));
        }
        let output = self.runner.run("git", &args)?;
        if !output.status.success() {
            anyhow::bail!(
                "git worktree add failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }

    fn branch_exists(&self, repo_root: &Path, branch: &str) -> anyhow::Result<bool> {
        let args = vec![
            "-C".to_string(),
            repo_root.to_string_lossy().to_string(),
            "rev-parse".to_string(),
            "--verify".to_string(),
            format!("refs/heads/{}", branch),
        ];
        let output = self.runner.run("git", &args)?;
        Ok(output.status.success())
    }

    fn remote_branch_exists(&self, repo_root: &Path, branch: &str) -> anyhow::Result<bool> {
        let args = vec![
            "-C".to_string(),
            repo_root.to_string_lossy().to_string(),
            "ls-remote".to_string(),
            "--exit-code".to_string(),
            "--heads".to_string(),
            "origin".to_string(),
            branch.to_string(),
        ];
        let output = self.runner.run("git", &args)?;
        Ok(output.status.success())
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
        self.config
            .data_dir
            .join("transcripts")
            .join(format!("{}.log", id.as_str()))
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

    fn log_state_change(
        &self,
        id: &forgemux_core::SessionId,
        from: SessionState,
        to: SessionState,
    ) {
        let record = serde_json::json!({
            "ts": Utc::now(),
            "type": "state-change",
            "session_id": id.as_str(),
            "from": format!("{from:?}"),
            "to": format!("{to:?}"),
        });
        self.log_event(id, record);
    }

    fn log_event(&self, id: &forgemux_core::SessionId, record: serde_json::Value) {
        let path = self
            .config
            .data_dir
            .join("events")
            .join(format!("{}.jsonl", id.as_str()));
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(line) = serde_json::to_string(&record)
            && let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path)
        {
            let _ = writeln!(file, "{line}");
        }
    }

    fn replay_path(&self, id: &forgemux_core::SessionId) -> PathBuf {
        self.config
            .data_dir
            .join("replay")
            .join(format!("{}.jsonl", id.as_str()))
    }

    pub fn record_replay_event(
        &self,
        id: &forgemux_core::SessionId,
        event_type: ReplayEventType,
        action: impl Into<String>,
        result: Option<String>,
        repo_id: Option<String>,
    ) {
        let now = Utc::now();
        let elapsed = self
            .store
            .load(id)
            .map(|record| format_elapsed(now, record.created_at))
            .unwrap_or_else(|_| "0s".to_string());
        let event = ReplayEvent {
            id: now.timestamp_millis() as u64,
            session_id: id.as_str().to_string(),
            repo_id,
            timestamp: now,
            elapsed,
            event_type,
            action: action.into(),
            result,
            payload: None,
        };
        let path = self.replay_path(id);
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(line) = serde_json::to_string(&event)
            && let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path)
        {
            let _ = writeln!(file, "{line}");
        }
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

fn format_elapsed(now: DateTime<Utc>, created_at: DateTime<Utc>) -> String {
    let secs = (now - created_at).num_seconds().max(0);
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
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
    rate_limit: std::sync::Mutex<HashMap<String, VecDeque<DateTime<Utc>>>>,
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
        data_dir: &Path,
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
        let Some(event) = event else {
            return;
        };

        if !self.should_fire(session, event, config.debounce_secs) {
            return;
        }
        if !self.should_rate_limit(session, config.rate_limit_per_hour) {
            return;
        }

        let hooks = match event {
            NotificationEvent::WaitingInput => &config.on_waiting_input,
            NotificationEvent::Errored => &config.on_error,
            NotificationEvent::IdleTimeout => &config.on_idle_timeout,
        };

        for hook in hooks {
            if let Some(kinds) = allowed
                && !kinds.contains(&hook.kind())
            {
                continue;
            }
            let result = send_hook_with_retry(runner, data_dir, hook, session, event);
            if result.is_ok() {
                break;
            }
        }
    }

    fn should_fire(
        &self,
        session: &SessionRecord,
        event: NotificationEvent,
        debounce: i64,
    ) -> bool {
        let now = Utc::now();
        let key = (session.id.as_str().to_string(), event);
        let mut guard = self.last_fired.lock().unwrap();
        if let Some(last) = guard.get(&key)
            && (now - *last).num_seconds() < debounce
        {
            return false;
        }
        guard.insert(key, now);
        true
    }

    fn should_rate_limit(&self, session: &SessionRecord, limit_per_hour: u32) -> bool {
        if limit_per_hour == 0 {
            return false;
        }
        let now = Utc::now();
        let window_start = now - chrono::Duration::hours(1);
        let mut guard = self.rate_limit.lock().unwrap();
        let entry = guard.entry(session.id.as_str().to_string()).or_default();
        while let Some(front) = entry.front() {
            if *front >= window_start {
                break;
            }
            entry.pop_front();
        }
        if entry.len() as u32 >= limit_per_hour {
            return false;
        }
        entry.push_back(now);
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
fn send_hook_with_retry<R: CommandRunner>(
    runner: &R,
    data_dir: &Path,
    hook: &NotificationHook,
    session: &SessionRecord,
    event: NotificationEvent,
) -> anyhow::Result<()> {
    match hook {
        NotificationHook::Desktop => {
            let message = render_template("Session {{session_id}} is {{state}}", session, event);
            let args = vec!["-a".to_string(), "forgemux".to_string(), message];
            let output = runner.run("notify-send", &args)?;
            let result = output.status.success();
            log_delivery(
                data_dir,
                session,
                event,
                hook.kind(),
                1,
                result,
                if result {
                    None
                } else {
                    Some("notify-send failed".to_string())
                },
            );
            if !result {
                anyhow::bail!("notify-send failed");
            }
        }
        NotificationHook::Command { program, args } => {
            let rendered = args
                .iter()
                .map(|arg| render_template(arg, session, event))
                .collect::<Vec<_>>();
            let output = runner.run(program, &rendered)?;
            let result = output.status.success();
            log_delivery(
                data_dir,
                session,
                event,
                hook.kind(),
                1,
                result,
                if result {
                    None
                } else {
                    Some("command failed".to_string())
                },
            );
            if !result {
                anyhow::bail!("command failed");
            }
        }
        NotificationHook::Webhook { url, template } => {
            let body = render_template(template, session, event);
            let client = reqwest::blocking::Client::new();
            let backoff = [1u64, 5u64, 15u64];
            for (idx, delay) in backoff.iter().enumerate() {
                if idx > 0 {
                    std::thread::sleep(std::time::Duration::from_secs(*delay));
                }
                let attempt = idx as u32 + 1;
                let response = client.post(url).body(body.clone()).send();
                match response {
                    Ok(resp) if resp.status().is_success() => {
                        log_delivery(data_dir, session, event, hook.kind(), attempt, true, None);
                        return Ok(());
                    }
                    Ok(resp) => {
                        let err = format!("webhook returned {}", resp.status());
                        log_delivery(
                            data_dir,
                            session,
                            event,
                            hook.kind(),
                            attempt,
                            false,
                            Some(err.clone()),
                        );
                        if attempt == backoff.len() as u32 {
                            anyhow::bail!(err);
                        }
                    }
                    Err(err) => {
                        let err_msg = err.to_string();
                        log_delivery(
                            data_dir,
                            session,
                            event,
                            hook.kind(),
                            attempt,
                            false,
                            Some(err_msg.clone()),
                        );
                        if attempt == backoff.len() as u32 {
                            anyhow::bail!(err_msg);
                        }
                    }
                }
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

#[derive(serde::Serialize)]
struct NotificationDelivery {
    ts: DateTime<Utc>,
    session_id: String,
    event: String,
    hook: NotificationKind,
    attempt: u32,
    success: bool,
    error: Option<String>,
}

fn log_delivery(
    data_dir: &Path,
    session: &SessionRecord,
    event: NotificationEvent,
    hook: NotificationKind,
    attempt: u32,
    success: bool,
    error: Option<String>,
) {
    let path = data_dir
        .join("notifications")
        .join("logs")
        .join(format!("{}.jsonl", session.id.as_str()));
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let event_name = match event {
        NotificationEvent::WaitingInput => "waiting".to_string(),
        NotificationEvent::Errored => "errored".to_string(),
        NotificationEvent::IdleTimeout => "terminated".to_string(),
    };
    let record = NotificationDelivery {
        ts: Utc::now(),
        session_id: session.id.as_str().to_string(),
        event: event_name.clone(),
        hook,
        attempt,
        success,
        error: error.clone(),
    };
    if let Ok(line) = serde_json::to_string(&record)
        && let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path)
    {
        let _ = writeln!(file, "{line}");
    }
    let event_record = serde_json::json!({
        "ts": record.ts,
        "type": "notification-delivery",
        "session_id": session.id.as_str(),
        "event": event_name,
        "hook": format!("{:?}", hook),
        "attempt": attempt,
        "success": success,
        "error": error,
    });
    let event_path = data_dir
        .join("events")
        .join(format!("{}.jsonl", session.id.as_str()));
    if let Some(parent) = event_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(line) = serde_json::to_string(&event_record)
        && let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(event_path)
    {
        let _ = writeln!(file, "{line}");
    }
}

fn expand_tilde(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/")
        && let Ok(home) = std::env::var("HOME")
    {
        return PathBuf::from(home).join(stripped);
    }
    PathBuf::from(path)
}

fn pid_is_alive(pid: u32) -> bool {
    PathBuf::from("/proc").join(pid.to_string()).exists()
}

fn is_jsonl(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("jsonl"))
        .unwrap_or(false)
}

fn find_latest_jsonl(dir: &Path) -> Option<PathBuf> {
    let mut newest: Option<(SystemTime, PathBuf)> = None;
    for entry in walkdir::WalkDir::new(dir)
        .max_depth(3)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
    {
        let path = entry.path();
        if !is_jsonl(path) {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        match &newest {
            Some((seen, _)) if *seen >= modified => {}
            _ => {
                newest = Some((modified, path.to_path_buf()));
            }
        }
    }
    newest.map(|(_, path)| path)
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
    fn start_session_sets_tmux_env() {
        let tmp = tempfile::tempdir().unwrap();
        let mut config = ForgedConfig::default_with_data_dir(tmp.path().to_path_buf());
        config.tmux_bin = "tmux".to_string();
        config.advertise_addr = Some("127.0.0.1:9999".to_string());
        let runner = FakeRunner::default();
        let service = SessionService::new(config, runner.clone());

        let record = service
            .start_session(AgentType::Claude, "sonnet", tmp.path())
            .unwrap();

        let calls = runner.calls();
        assert!(calls.iter().any(|call| {
            call.contains(&"set-environment".to_string())
                && call.contains(&record.id.as_str().to_string())
                && call.contains(&"SESSION_ID".to_string())
        }));
        assert!(calls.iter().any(|call| {
            call.contains(&"set-environment".to_string())
                && call.contains(&"FORGED_PORT".to_string())
                && call.contains(&"9999".to_string())
        }));
    }

    #[test]
    fn start_session_writes_state_change_log() {
        let tmp = tempfile::tempdir().unwrap();
        let mut config = ForgedConfig::default_with_data_dir(tmp.path().to_path_buf());
        config.tmux_bin = "tmux".to_string();
        let runner = FakeRunner::default();
        let service = SessionService::new(config, runner);

        let record = service
            .start_session(AgentType::Claude, "sonnet", tmp.path())
            .unwrap();

        let path = tmp
            .path()
            .join("events")
            .join(format!("{}.jsonl", record.id.as_str()));
        let content = std::fs::read_to_string(path).unwrap();
        assert!(content.contains("\"type\":\"state-change\""));
        assert!(content.contains("\"to\":\"Running\""));
    }

    #[test]
    fn start_session_writes_replay_event() {
        let tmp = tempfile::tempdir().unwrap();
        let mut config = ForgedConfig::default_with_data_dir(tmp.path().to_path_buf());
        config.tmux_bin = "tmux".to_string();
        let runner = FakeRunner::default();
        let service = SessionService::new(config, runner);

        let record = service
            .start_session(AgentType::Claude, "sonnet", tmp.path())
            .unwrap();

        let replay_path = tmp
            .path()
            .join("replay")
            .join(format!("{}.jsonl", record.id.as_str()));
        let content = std::fs::read_to_string(replay_path).unwrap();
        assert!(content.contains("\"event_type\":\"system\""));
        assert!(content.contains("Session started"));
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
    fn stop_session_writes_state_change_log() {
        let tmp = tempfile::tempdir().unwrap();
        let config = ForgedConfig::default_with_data_dir(tmp.path().to_path_buf());
        let runner = FakeRunner::default();
        let service = SessionService::new(config, runner);

        let mut record = SessionRecord::new(AgentType::Claude, "sonnet", tmp.path().to_path_buf());
        record.id = forgemux_core::SessionId::from("S-STOP1");
        record.touch_state(SessionState::Running);
        service.store.save(&record).unwrap();

        service.stop_session(record.id.as_str()).unwrap();

        let path = tmp
            .path()
            .join("events")
            .join(format!("{}.jsonl", record.id.as_str()));
        let content = std::fs::read_to_string(path).unwrap();
        assert!(content.contains("\"to\":\"Terminated\""));
    }

    #[test]
    fn notification_engine_debounces() {
        let engine = NotificationEngine::new();
        let mut record = SessionRecord::new(AgentType::Claude, "sonnet", PathBuf::from("/tmp"));
        record.id = forgemux_core::SessionId::from("S-0001");
        let tmp = tempfile::tempdir().unwrap();

        let config = NotificationConfig {
            on_waiting_input: vec![NotificationHook::Desktop],
            on_error: vec![],
            on_idle_timeout: vec![],
            debounce_secs: 60,
            rate_limit_per_hour: 1,
        };

        let runner = FakeRunner::default();
        engine.maybe_notify(
            &config,
            &runner,
            tmp.path(),
            &record,
            &SessionState::WaitingInput,
            None,
        );
        engine.maybe_notify(
            &config,
            &runner,
            tmp.path(),
            &record,
            &SessionState::WaitingInput,
            None,
        );

        let calls = runner.calls();
        assert_eq!(calls.len(), 1);
    }

    #[test]
    fn notification_engine_rate_limits() {
        let engine = NotificationEngine::new();
        let mut record = SessionRecord::new(AgentType::Claude, "sonnet", PathBuf::from("/tmp"));
        record.id = forgemux_core::SessionId::from("S-0005");
        let tmp = tempfile::tempdir().unwrap();

        let config = NotificationConfig {
            on_waiting_input: vec![NotificationHook::Command {
                program: "echo".to_string(),
                args: vec!["hi".to_string()],
            }],
            on_error: vec![],
            on_idle_timeout: vec![],
            debounce_secs: 0,
            rate_limit_per_hour: 1,
        };

        let runner = FakeRunner::default();
        engine.maybe_notify(
            &config,
            &runner,
            tmp.path(),
            &record,
            &SessionState::WaitingInput,
            None,
        );
        engine.maybe_notify(
            &config,
            &runner,
            tmp.path(),
            &record,
            &SessionState::WaitingInput,
            None,
        );

        let calls = runner.calls();
        assert_eq!(calls.len(), 1);
    }

    #[test]
    fn render_template_expands_session_values() {
        let record = SessionRecord::new(AgentType::Codex, "o3", PathBuf::from("/tmp"));
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
        let mut record = SessionRecord::new(AgentType::Claude, "sonnet", PathBuf::from("/tmp"));
        record.id = forgemux_core::SessionId::from("S-0002");
        let tmp = tempfile::tempdir().unwrap();

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
            rate_limit_per_hour: 60,
        };

        let runner = FakeRunner::default();
        engine.maybe_notify(
            &config,
            &runner,
            tmp.path(),
            &record,
            &SessionState::WaitingInput,
            Some(&[NotificationKind::Command]),
        );

        let calls = runner.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0][0], "echo");
    }

    #[test]
    fn notification_delivery_writes_log() {
        let engine = NotificationEngine::new();
        let mut record = SessionRecord::new(AgentType::Claude, "sonnet", PathBuf::from("/tmp"));
        record.id = forgemux_core::SessionId::from("S-0003");
        let tmp = tempfile::tempdir().unwrap();

        let config = NotificationConfig {
            on_waiting_input: vec![NotificationHook::Command {
                program: "echo".to_string(),
                args: vec!["ok".to_string()],
            }],
            on_error: vec![],
            on_idle_timeout: vec![],
            debounce_secs: 0,
            rate_limit_per_hour: 60,
        };

        let runner = FakeRunner::default();
        engine.maybe_notify(
            &config,
            &runner,
            tmp.path(),
            &record,
            &SessionState::WaitingInput,
            None,
        );

        let log_path = tmp
            .path()
            .join("notifications")
            .join("logs")
            .join("S-0003.jsonl");
        let content = std::fs::read_to_string(log_path).unwrap();
        assert!(content.contains("\"success\":true"));
        assert!(content.contains("\"event\":\"waiting\""));

        let event_path = tmp.path().join("events").join("S-0003.jsonl");
        let events = std::fs::read_to_string(event_path).unwrap();
        assert!(events.contains("\"type\":\"notification-delivery\""));
    }

    #[test]
    fn notification_falls_back_on_failure() {
        let engine = NotificationEngine::new();
        let mut record = SessionRecord::new(AgentType::Claude, "sonnet", PathBuf::from("/tmp"));
        record.id = forgemux_core::SessionId::from("S-0004");
        let tmp = tempfile::tempdir().unwrap();

        let config = NotificationConfig {
            on_waiting_input: vec![
                NotificationHook::Command {
                    program: "fail".to_string(),
                    args: vec!["no".to_string()],
                },
                NotificationHook::Command {
                    program: "echo".to_string(),
                    args: vec!["ok".to_string()],
                },
            ],
            on_error: vec![],
            on_idle_timeout: vec![],
            debounce_secs: 0,
            rate_limit_per_hour: 60,
        };

        let runner = FakeRunner::default();
        runner.set_status_for(&["fail"], 1);
        engine.maybe_notify(
            &config,
            &runner,
            tmp.path(),
            &record,
            &SessionState::WaitingInput,
            None,
        );

        let calls = runner.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0][0], "fail");
        assert_eq!(calls[1][0], "echo");
    }

    #[test]
    fn refresh_states_uses_agent_log_waiting_hint() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("project");
        std::fs::create_dir_all(&repo).unwrap();
        let logs_root = tmp.path().join("logs");
        let project_logs = logs_root.join("project");
        std::fs::create_dir_all(&project_logs).unwrap();
        let log_file = project_logs.join("session.jsonl");
        std::fs::write(&log_file, r#"{"type":"permission_request"}"#).unwrap();

        let mut config = ForgedConfig::default_with_data_dir(tmp.path().to_path_buf());
        if let Some(agent) = config.agents.get_mut(&AgentType::Claude) {
            agent.usage_paths = vec![logs_root.to_string_lossy().to_string()];
        }
        let runner = FakeRunner::default();
        let service = SessionService::new(config, runner);

        let mut record = service
            .manager
            .create_session(AgentType::Claude, "sonnet", &repo)
            .unwrap();
        record.touch_state(SessionState::Running);
        service.store.save(&record).unwrap();

        let sessions = service.refresh_states().unwrap();
        let updated = sessions.iter().find(|s| s.id == record.id).unwrap();
        assert_eq!(updated.state, SessionState::WaitingInput);
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
        runner.set_status_for(
            &["git", "rev-parse", "--verify", "refs/heads/test-branch"],
            1,
        );
        let service = SessionService::new(config, runner.clone());

        let worktree_path = tmp.path().join("wt");
        service
            .create_worktree(&repo, &worktree_path, "test-branch")
            .unwrap();

        let calls = runner.calls();
        assert!(calls.iter().any(|call| {
            call.contains(&"git".to_string())
                && call.contains(&"fetch".to_string())
                && call.contains(&"--prune".to_string())
        }));
        assert!(calls.iter().any(|call| {
            call.contains(&"git".to_string())
                && call.contains(&"ls-remote".to_string())
                && call.contains(&"origin".to_string())
        }));
        assert!(calls.iter().any(|call| {
            call.contains(&"git".to_string())
                && call.contains(&"worktree".to_string())
                && call.contains(&"add".to_string())
                && call.contains(&"-b".to_string())
                && call.contains(&"origin/test-branch".to_string())
        }));
    }

    #[test]
    fn create_worktree_reuses_existing_path() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let status = Command::new("git").arg("init").arg(&repo).status().unwrap();
        assert!(status.success());

        let config = ForgedConfig::default_with_data_dir(tmp.path().to_path_buf());
        let runner = FakeRunner::default();
        let service = SessionService::new(config, runner.clone());

        let worktree_path = tmp.path().join("wt");
        std::fs::create_dir_all(&worktree_path).unwrap();

        service
            .create_worktree(&repo, &worktree_path, "test-branch")
            .unwrap();

        let calls = runner.calls();
        assert!(calls.is_empty());
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
        assert!(
            calls
                .iter()
                .any(|call| call.contains(&"send-keys".to_string()))
        );
    }

    #[test]
    fn send_keys_logs_input_event() {
        let tmp = tempfile::tempdir().unwrap();
        let config = ForgedConfig::default_with_data_dir(tmp.path().to_path_buf());
        let runner = FakeRunner::default();
        let service = SessionService::new(config, runner);

        let id = forgemux_core::SessionId::from("S-INPUT1");
        service.send_keys(&id, "ls\n").unwrap();

        let path = tmp
            .path()
            .join("events")
            .join(format!("{}.jsonl", id.as_str()));
        let content = std::fs::read_to_string(path).unwrap();
        assert!(content.contains("\"type\":\"user-input\""));
        assert!(content.contains("\"bytes\":3"));
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
                None,
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
    fn foreman_report_includes_sessions() {
        let tmp = tempfile::tempdir().unwrap();
        let config = ForgedConfig::default_with_data_dir(tmp.path().to_path_buf());
        let runner = FakeRunner::default();
        let service = SessionService::new(config, runner);

        let _ = service
            .start_session(AgentType::Claude, "sonnet", tmp.path())
            .unwrap();
        let report = service.foreman_report().unwrap();
        assert_eq!(report.sessions.len(), 1);
    }

    #[test]
    fn start_session_rejects_unknown_policy() {
        let tmp = tempfile::tempdir().unwrap();
        let config = ForgedConfig::default_with_data_dir(tmp.path().to_path_buf());
        let runner = FakeRunner::default();
        let service = SessionService::new(config, runner);

        let result = service.start_session_with_worktree(
            AgentType::Claude,
            "sonnet",
            tmp.path(),
            None,
            None,
            Some("restricted".to_string()),
        );
        assert!(result.is_err());
    }

    #[test]
    fn start_session_uses_default_repo_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();

        let mut config = ForgedConfig::default_with_data_dir(tmp.path().to_path_buf());
        config.default_repo = Some(repo.clone());
        let runner = FakeRunner::default();
        let service = SessionService::new(config, runner);

        let record = service
            .start_session_with_worktree(AgentType::Claude, "sonnet", "", None, None, None)
            .unwrap();
        assert_eq!(record.repo_root, repo);
    }

    #[test]
    fn usage_defaults_to_zero() {
        let tmp = tempfile::tempdir().unwrap();
        let config = ForgedConfig::default_with_data_dir(tmp.path().to_path_buf());
        let runner = FakeRunner::default();
        let service = SessionService::new(config, runner);

        let record = service.usage("S-TEST").unwrap();
        assert_eq!(record.total_tokens, 0);
        assert_eq!(record.estimated_cost_usd, 0.0);
    }

    #[test]
    fn drain_blocks_new_sessions() {
        let tmp = tempfile::tempdir().unwrap();
        let config = ForgedConfig::default_with_data_dir(tmp.path().to_path_buf());
        let runner = FakeRunner::default();
        let service = SessionService::new(config, runner);

        service.drain(false).unwrap();
        let result = service.start_session(AgentType::Claude, "sonnet", tmp.path());
        assert!(result.is_err());
    }

    #[test]
    fn pid_lock_prevents_second_start() {
        let tmp = tempfile::tempdir().unwrap();
        let config = ForgedConfig::default_with_data_dir(tmp.path().to_path_buf());
        let runner = FakeRunner::default();
        let service = SessionService::new(config, runner);

        let _lock = service.acquire_pid_lock().unwrap();
        let err = service.acquire_pid_lock().unwrap_err();
        assert!(err.to_string().contains("forged already running"));
    }

    #[test]
    fn pid_lock_cleans_up_on_drop() {
        let tmp = tempfile::tempdir().unwrap();
        let config = ForgedConfig::default_with_data_dir(tmp.path().to_path_buf());
        let runner = FakeRunner::default();
        let service = SessionService::new(config, runner);
        let path = service.config.data_dir.join("forged.pid");

        {
            let _lock = service.acquire_pid_lock().unwrap();
            assert!(path.exists());
        }

        assert!(!path.exists());
    }

    #[test]
    fn cleanup_orphan_sessions_kills_unknown_tmux_sessions() {
        let tmp = tempfile::tempdir().unwrap();
        let config = ForgedConfig::default_with_data_dir(tmp.path().to_path_buf());
        let runner = FakeRunner::default();
        runner.set_stdout_for(
            &["list-sessions", "-F", "#{session_name}"],
            b"S-keep\nS-orphan\nnot-forgemux\n",
        );
        let service = SessionService::new(config, runner.clone());

        let mut record = service
            .manager
            .create_session(AgentType::Claude, "sonnet", tmp.path())
            .unwrap();
        record.id = forgemux_core::SessionId::from("S-keep");
        service.store.save(&record).unwrap();

        let reaped = service.cleanup_orphan_sessions().unwrap();
        assert_eq!(reaped, 1);

        let calls = runner.calls();
        assert!(calls.iter().any(|call| {
            call.contains(&"kill-session".to_string()) && call.contains(&"S-orphan".to_string())
        }));
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
snapshot_interval_ms = 999
stream_encryption_key = "base64key"
api_tokens = ["token-1", "token-2"]
hub_token = "hub-secret"
default_repo = "/tmp/project"
 
[policies.restricted]
cpu_shares = 512
memory_max = "1G"
pids_max = 128
network = "none"

[agents.claude]
command = "claude-custom"
args = ["--model", "haiku"]
prompt_patterns = ["(?m)^>$"]

[notifications]
debounce_secs = 42
rate_limit_per_hour = 5

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
        assert_eq!(config.hub_token.as_deref(), Some("hub-secret"));
        assert_eq!(config.advertise_addr.as_deref(), Some("edge-01.local:9443"));
        assert_eq!(config.event_ring_capacity, 42);
        assert_eq!(config.input_dedup_window, 55);
        assert_eq!(config.snapshot_lines, 123);
        assert_eq!(config.poll_interval_ms, 10);
        assert_eq!(config.snapshot_interval_ms, 999);
        assert_eq!(config.stream_encryption_key.as_deref(), Some("base64key"));
        assert!(config.policies.contains_key("restricted"));
        assert_eq!(config.api_tokens.len(), 2);
        assert_eq!(config.default_repo, Some(PathBuf::from("/tmp/project")));
        let claude = config.agents.get(&AgentType::Claude).unwrap();
        assert_eq!(claude.command, "claude-custom");
        assert_eq!(
            claude.args,
            vec!["--model".to_string(), "haiku".to_string()]
        );
        assert_eq!(claude.prompt_patterns, vec!["(?m)^>$".to_string()]);
        assert_eq!(config.notifications.debounce_secs, 42);
        assert_eq!(config.notifications.rate_limit_per_hour, 5);
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
