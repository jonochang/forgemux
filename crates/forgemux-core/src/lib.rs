use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("session not found: {0}")]
    SessionNotFound(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AgentType {
    Claude,
    Codex,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SessionState {
    Provisioning,
    Starting,
    Running,
    Idle,
    WaitingInput,
    Errored,
    Terminated,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SessionRole {
    Worker,
    Foreman {
        watch_scope: Vec<String>,
        intervention: InterventionLevel,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum InterventionLevel {
    Advisory,
    Assisted,
    Autonomous,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionId(String);

impl SessionId {
    pub fn new() -> Self {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let short = &id[..4];
        Self(format!("S-{}", short))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for SessionId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<&str> for SessionId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionRecord {
    pub id: SessionId,
    pub agent: AgentType,
    pub model: String,
    pub repo_root: PathBuf,
    pub state: SessionState,
    pub role: SessionRole,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_activity_at: DateTime<Utc>,
}

impl SessionRecord {
    pub fn new(agent: AgentType, model: impl Into<String>, repo_root: PathBuf) -> Self {
        let now = Utc::now();
        Self {
            id: SessionId::new(),
            agent,
            model: model.into(),
            repo_root,
            state: SessionState::Provisioning,
            role: SessionRole::Worker,
            created_at: now,
            updated_at: now,
            last_activity_at: now,
        }
    }

    pub fn touch_state(&mut self, state: SessionState) {
        let now = Utc::now();
        self.state = state;
        self.updated_at = now;
        self.last_activity_at = now;
    }
}

#[derive(Debug, Clone)]
pub struct SessionStore {
    root: PathBuf,
}

impl SessionStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn ensure_dirs(&self) -> Result<(), CoreError> {
        fs::create_dir_all(self.sessions_dir())?;
        Ok(())
    }

    pub fn save(&self, record: &SessionRecord) -> Result<(), CoreError> {
        self.ensure_dirs()?;
        let path = self.session_path(&record.id);
        let data = serde_json::to_vec_pretty(record)?;
        fs::write(path, data)?;
        Ok(())
    }

    pub fn load(&self, id: &SessionId) -> Result<SessionRecord, CoreError> {
        let path = self.session_path(id);
        if !path.exists() {
            return Err(CoreError::SessionNotFound(id.to_string()));
        }
        let data = fs::read(path)?;
        Ok(serde_json::from_slice(&data)?)
    }

    pub fn list(&self) -> Result<Vec<SessionRecord>, CoreError> {
        let dir = self.sessions_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut records = Vec::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                let data = fs::read(entry.path())?;
                let record: SessionRecord = serde_json::from_slice(&data)?;
                records.push(record);
            }
        }
        Ok(records)
    }

    fn sessions_dir(&self) -> PathBuf {
        self.root.join("sessions")
    }

    fn session_path(&self, id: &SessionId) -> PathBuf {
        self.sessions_dir().join(format!("{}.json", id.as_str()))
    }
}

#[derive(Debug, Clone)]
pub struct RepoRoot(PathBuf);

impl RepoRoot {
    pub fn discover(start: impl AsRef<Path>) -> Option<Self> {
        let mut current = start.as_ref();
        loop {
            if current.join(".git").exists() {
                return Some(Self(current.to_path_buf()));
            }
            match current.parent() {
                Some(parent) => current = parent,
                None => return None,
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct StateDetector {
    idle_threshold_secs: i64,
    waiting_threshold_secs: i64,
    prompt_patterns: Vec<Regex>,
}

#[derive(Debug, Clone)]
pub struct StateSignal {
    pub process_alive: bool,
    pub exit_code: Option<i32>,
    pub last_output_at: DateTime<Utc>,
    pub recent_output: String,
}

impl StateDetector {
    pub fn new(
        idle_threshold_secs: i64,
        waiting_threshold_secs: i64,
        prompt_patterns: Vec<Regex>,
    ) -> Self {
        Self {
            idle_threshold_secs,
            waiting_threshold_secs,
            prompt_patterns,
        }
    }

    pub fn detect(&self, now: DateTime<Utc>, signal: &StateSignal) -> SessionState {
        if !signal.process_alive {
            return match signal.exit_code {
                Some(code) if code == 0 => SessionState::Terminated,
                Some(_) => SessionState::Errored,
                None => SessionState::Errored,
            };
        }

        let idle_secs = (now - signal.last_output_at).num_seconds();
        let waiting_prompt = self
            .prompt_patterns
            .iter()
            .any(|pat| pat.is_match(&signal.recent_output));

        if waiting_prompt && idle_secs >= self.waiting_threshold_secs {
            return SessionState::WaitingInput;
        }

        if idle_secs >= self.idle_threshold_secs {
            return SessionState::Idle;
        }

        SessionState::Running
    }
}

#[derive(Debug, Clone)]
pub struct SessionManager {
    store: SessionStore,
}

impl SessionManager {
    pub fn new(store: SessionStore) -> Self {
        Self { store }
    }

    pub fn create_session(
        &self,
        agent: AgentType,
        model: impl Into<String>,
        repo_path: impl AsRef<Path>,
    ) -> Result<SessionRecord, CoreError> {
        self.create_session_with_role(agent, model, repo_path, SessionRole::Worker)
    }

    pub fn create_session_with_role(
        &self,
        agent: AgentType,
        model: impl Into<String>,
        repo_path: impl AsRef<Path>,
        role: SessionRole,
    ) -> Result<SessionRecord, CoreError> {
        let canonical = fs::canonicalize(repo_path.as_ref())?;
        let repo_root = RepoRoot::discover(&canonical)
            .map(|root| root.path().to_path_buf())
            .unwrap_or(canonical);
        let mut record = SessionRecord::new(agent, model, repo_root);
        record.role = role;
        self.store.save(&record)?;
        Ok(record)
    }
}

pub fn sort_sessions(mut sessions: Vec<SessionRecord>) -> Vec<SessionRecord> {
    sessions.sort_by(|a, b| {
        let pa = state_priority(&a.state);
        let pb = state_priority(&b.state);
        pa.cmp(&pb)
            .then_with(|| b.last_activity_at.cmp(&a.last_activity_at))
    });
    sessions
}

fn state_priority(state: &SessionState) -> u8 {
    match state {
        SessionState::WaitingInput => 0,
        SessionState::Running => 1,
        SessionState::Idle => 2,
        SessionState::Errored => 3,
        SessionState::Terminated => 4,
        SessionState::Provisioning => 5,
        SessionState::Starting => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn session_id_has_prefix() {
        let id = SessionId::new();
        assert!(id.as_str().starts_with("S-"));
        assert_eq!(id.as_str().len(), 6);
    }

    #[test]
    fn session_store_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SessionStore::new(tmp.path());
        let mut record = SessionRecord::new(
            AgentType::Claude,
            "sonnet",
            tmp.path().to_path_buf(),
        );
        record.touch_state(SessionState::Running);
        store.save(&record).unwrap();

        let loaded = store.load(&record.id).unwrap();
        assert_eq!(loaded.id, record.id);
        assert_eq!(loaded.state, SessionState::Running);
    }

    #[test]
    fn repo_root_discovers_git_root() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_path = tmp.path().join("repo");
        std::fs::create_dir_all(&repo_path).unwrap();
        let status = Command::new("git")
            .arg("init")
            .arg(&repo_path)
            .status()
            .unwrap();
        assert!(status.success());

        let nested = repo_path.join("nested");
        std::fs::create_dir_all(&nested).unwrap();

        let root = RepoRoot::discover(&nested).unwrap();
        assert_eq!(root.path(), repo_path.as_path());
    }

    #[test]
    fn session_manager_uses_worktree_root() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_path = tmp.path().join("repo");
        init_git_repo(&repo_path);

        let worktree = tmp.path().join("worktree");
        let status = Command::new("git")
            .arg("-C")
            .arg(&repo_path)
            .arg("worktree")
            .arg("add")
            .arg(&worktree)
            .status()
            .unwrap();
        assert!(status.success());

        let nested = worktree.join("nested");
        std::fs::create_dir_all(&nested).unwrap();

        let store = SessionStore::new(tmp.path().join("store"));
        let manager = SessionManager::new(store);
        let record = manager
            .create_session(AgentType::Claude, "sonnet", &nested)
            .unwrap();

        assert_eq!(record.repo_root, worktree.canonicalize().unwrap());
    }

    #[test]
    fn session_manager_falls_back_to_path_when_not_git() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_path = tmp.path().join("plain");
        std::fs::create_dir_all(&repo_path).unwrap();

        let store = SessionStore::new(tmp.path().join("store"));
        let manager = SessionManager::new(store);
        let record = manager
            .create_session(AgentType::Codex, "o3", &repo_path)
            .unwrap();

        assert_eq!(record.repo_root, repo_path.canonicalize().unwrap());
    }

    fn init_git_repo(path: &Path) {
        std::fs::create_dir_all(path).unwrap();
        let status = Command::new("git")
            .arg("init")
            .arg(path)
            .status()
            .unwrap();
        assert!(status.success());

        let readme = path.join("README.md");
        std::fs::write(&readme, "test").unwrap();

        let status = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(["add", "."])
            .status()
            .unwrap();
        assert!(status.success());

        let status = Command::new("git")
            .arg("-C")
            .arg(path)
            .args([
                "-c",
                "user.name=forgemux",
                "-c",
                "user.email=forgemux@example.com",
                "commit",
                "-m",
                "init",
            ])
            .status()
            .unwrap();
        assert!(status.success());

    }

    #[test]
    fn state_detector_marks_waiting_input() {
        let detector = StateDetector::new(
            60,
            10,
            vec![Regex::new(r\"(?m)^>\\s*$\").unwrap()],
        );
        let signal = StateSignal {
            process_alive: true,
            exit_code: None,
            last_output_at: Utc::now() - chrono::Duration::seconds(15),
            recent_output: \">\".to_string(),
        };
        let state = detector.detect(Utc::now(), &signal);
        assert_eq!(state, SessionState::WaitingInput);
    }

    #[test]
    fn state_detector_marks_idle() {
        let detector = StateDetector::new(30, 10, vec![]);
        let signal = StateSignal {
            process_alive: true,
            exit_code: None,
            last_output_at: Utc::now() - chrono::Duration::seconds(45),
            recent_output: \"\".to_string(),
        };
        let state = detector.detect(Utc::now(), &signal);
        assert_eq!(state, SessionState::Idle);
    }

    #[test]
    fn state_detector_marks_running() {
        let detector = StateDetector::new(30, 10, vec![]);
        let signal = StateSignal {
            process_alive: true,
            exit_code: None,
            last_output_at: Utc::now() - chrono::Duration::seconds(5),
            recent_output: \"\".to_string(),
        };
        let state = detector.detect(Utc::now(), &signal);
        assert_eq!(state, SessionState::Running);
    }

    #[test]
    fn state_detector_marks_errored() {
        let detector = StateDetector::new(30, 10, vec![]);
        let signal = StateSignal {
            process_alive: false,
            exit_code: Some(1),
            last_output_at: Utc::now(),
            recent_output: \"\".to_string(),
        };
        let state = detector.detect(Utc::now(), &signal);
        assert_eq!(state, SessionState::Errored);
    }

    #[test]
    fn sort_sessions_prioritizes_waiting_input() {
        let now = Utc::now();
        let mut waiting = SessionRecord::new(
            AgentType::Claude,
            "sonnet",
            PathBuf::from("/tmp"),
        );
        waiting.state = SessionState::WaitingInput;
        waiting.last_activity_at = now - chrono::Duration::seconds(300);

        let mut running = SessionRecord::new(
            AgentType::Claude,
            "sonnet",
            PathBuf::from("/tmp"),
        );
        running.state = SessionState::Running;
        running.last_activity_at = now;

        let sorted = sort_sessions(vec![running.clone(), waiting.clone()]);
        assert_eq!(sorted[0].state, SessionState::WaitingInput);
        assert_eq!(sorted[1].state, SessionState::Running);
    }
}
