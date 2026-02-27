use std::path::{Path, PathBuf};

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

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
}
