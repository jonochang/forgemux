use anyhow::Context;
use forgemux_core::{sort_sessions, SessionRecord, SessionStore};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubEdge {
    pub id: String,
    pub data_dir: PathBuf,
    pub ws_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubConfig {
    pub data_dir: PathBuf,
    pub edges: Vec<HubEdge>,
}

impl HubConfig {
    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let data = fs::read_to_string(path)?;
        let cfg = toml::from_str(&data)?;
        Ok(cfg)
    }
}

pub struct HubService {
    config: HubConfig,
}

impl HubService {
    pub fn new(config: HubConfig) -> Self {
        Self { config }
    }

    pub fn list_edges(&self) -> Vec<HubEdge> {
        self.config.edges.clone()
    }

    pub fn resolve_ws_url(&self, edge_id: Option<&str>) -> Option<String> {
        if let Some(id) = edge_id {
            return self
                .config
                .edges
                .iter()
                .find(|edge| edge.id == id)
                .and_then(|edge| edge.ws_url.clone());
        }
        if self.config.edges.len() == 1 {
            return self.config.edges[0].ws_url.clone();
        }
        None
    }

    pub fn list_sessions(&self) -> anyhow::Result<Vec<SessionRecord>> {
        let mut sessions = Vec::new();
        for edge in &self.config.edges {
            let store = SessionStore::new(&edge.data_dir);
            let mut edge_sessions = store
                .list()
                .with_context(|| format!("failed to read edge {}", edge.id))?;
            sessions.append(&mut edge_sessions);
        }
        Ok(sort_sessions(sessions))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use forgemux_core::{AgentType, SessionRecord, SessionState};

    #[test]
    fn hub_service_aggregates_sessions() {
        let tmp = tempfile::tempdir().unwrap();
        let edge1 = tmp.path().join("edge1");
        let edge2 = tmp.path().join("edge2");

        let store1 = SessionStore::new(&edge1);
        let mut s1 = SessionRecord::new(AgentType::Claude, "sonnet", edge1.clone());
        s1.state = SessionState::Running;
        store1.save(&s1).unwrap();

        let store2 = SessionStore::new(&edge2);
        let mut s2 = SessionRecord::new(AgentType::Codex, "o3", edge2.clone());
        s2.state = SessionState::WaitingInput;
        store2.save(&s2).unwrap();

        let service = HubService::new(HubConfig {
            data_dir: tmp.path().join("hub"),
            edges: vec![
                HubEdge {
                    id: "edge1".to_string(),
                    data_dir: edge1,
                    ws_url: None,
                },
                HubEdge {
                    id: "edge2".to_string(),
                    data_dir: edge2,
                    ws_url: None,
                },
            ],
        });

        let sessions = service.list_sessions().unwrap();
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].state, SessionState::WaitingInput);
    }
}
