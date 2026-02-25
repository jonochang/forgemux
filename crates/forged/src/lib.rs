use anyhow::Context;
use chrono::{DateTime, Utc};
use forgemux_core::{
    sort_sessions, AgentType, SessionManager, SessionRecord, SessionState, SessionStore, StateDetector,
    StateSignal,
};
use regex::Regex;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::SystemTime;

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

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub command: String,
    pub args: Vec<String>,
    pub prompt_patterns: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ForgedConfig {
    pub data_dir: PathBuf,
    pub tmux_bin: String,
    pub idle_threshold_secs: i64,
    pub waiting_threshold_secs: i64,
    pub agents: HashMap<AgentType, AgentConfig>,
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
        }
    }
}

pub struct SessionService<R: CommandRunner> {
    config: ForgedConfig,
    runner: R,
    store: SessionStore,
    manager: SessionManager,
}

impl<R: CommandRunner> SessionService<R> {
    pub fn new(config: ForgedConfig, runner: R) -> Self {
        let store = SessionStore::new(&config.data_dir);
        let manager = SessionManager::new(store.clone());
        Self {
            config,
            runner,
            store,
            manager,
        }
    }

    pub fn start_session(
        &self,
        agent: AgentType,
        model: impl Into<String>,
        repo_path: impl AsRef<Path>,
    ) -> anyhow::Result<SessionRecord> {
        let mut record = self
            .manager
            .create_session(agent.clone(), model, repo_path)
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
        let args = vec![
            "capture-pane".to_string(),
            "-p".to_string(),
            "-S".to_string(),
            "-100".to_string(),
            "-t".to_string(),
            id.as_str().to_string(),
        ];
        let output = self.runner.run(&self.config.tmux_bin, &args)?;
        if !output.status.success() {
            return Ok(String::new());
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
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
        let cmd = format!("cat >> {}", path.display());
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

    fn transcript_path(&self, id: &forgemux_core::SessionId) -> PathBuf {
        self.config.data_dir.join("transcripts").join(format!("{}.log", id.as_str()))
    }

    fn transcript_mtime(&self, id: &forgemux_core::SessionId) -> Option<DateTime<Utc>> {
        let path = self.transcript_path(id);
        let meta = fs::metadata(path).ok()?;
        let modified = meta.modified().ok()?;
        Some(system_time_to_chrono(modified))
    }
}

fn system_time_to_chrono(time: SystemTime) -> DateTime<Utc> {
    DateTime::<Utc>::from(time)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct FakeRunner {
        calls: Arc<Mutex<Vec<Vec<String>>>>,
        should_fail: bool,
    }

    impl FakeRunner {
        fn calls(&self) -> Vec<Vec<String>> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl CommandRunner for FakeRunner {
        fn run(&self, program: &str, args: &[String]) -> std::io::Result<Output> {
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
}
