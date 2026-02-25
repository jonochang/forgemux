use chrono::{DateTime, Utc};
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionRecord {
    pub id: SessionId,
    pub agent: AgentType,
    pub model: String,
    pub repo_root: PathBuf,
    pub state: SessionState,
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
        let canonical = fs::canonicalize(repo_path.as_ref())?;
        let repo_root = RepoRoot::discover(&canonical)
            .map(|root| root.path().to_path_buf())
            .unwrap_or(canonical);
        let record = SessionRecord::new(agent, model, repo_root);
        self.store.save(&record)?;
        Ok(record)
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
}
