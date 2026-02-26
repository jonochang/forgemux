use anyhow::Context;
use chrono::{DateTime, Utc};
use forgemux_core::{sort_sessions, SessionRecord, SessionStore};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubEdge {
    pub id: String,
    pub data_dir: PathBuf,
    pub ws_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeRegistration {
    pub id: String,
    pub addr: String,
    pub last_seen: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubConfig {
    pub data_dir: PathBuf,
    #[serde(default)]
    pub edges: Vec<HubEdge>,
    #[serde(default)]
    pub tokens: Vec<String>,
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
    registry: Arc<Mutex<HashMap<String, EdgeRegistration>>>,
    round_robin: Arc<Mutex<usize>>,
}

impl HubService {
    pub fn new(config: HubConfig) -> Self {
        Self {
            config,
            registry: Arc::new(Mutex::new(HashMap::new())),
            round_robin: Arc::new(Mutex::new(0)),
        }
    }

    pub fn list_edges(&self) -> Vec<HubEdge> {
        self.config.edges.clone()
    }

    pub fn tokens_required(&self) -> bool {
        !self.config.tokens.is_empty()
    }

    pub fn is_token_valid(&self, token: &str) -> bool {
        self.config.tokens.iter().any(|t| t == token)
    }

    pub fn register_edge(&self, id: String, addr: String) -> EdgeRegistration {
        let registration = EdgeRegistration {
            id: id.clone(),
            addr,
            last_seen: Utc::now(),
        };
        let mut guard = self.registry.lock().unwrap();
        guard.insert(id, registration.clone());
        registration
    }

    pub fn heartbeat(&self, id: &str) -> Option<EdgeRegistration> {
        let mut guard = self.registry.lock().unwrap();
        let entry = guard.get_mut(id)?;
        entry.last_seen = Utc::now();
        Some(entry.clone())
    }

    pub fn list_registered_edges(&self) -> Vec<EdgeRegistration> {
        let guard = self.registry.lock().unwrap();
        guard.values().cloned().collect()
    }

    pub fn pick_edge(&self) -> Option<EdgeRegistration> {
        let guard = self.registry.lock().unwrap();
        if guard.is_empty() {
            return None;
        }
        let mut idx = self.round_robin.lock().unwrap();
        let mut edges: Vec<_> = guard.values().cloned().collect();
        edges.sort_by(|a, b| a.id.cmp(&b.id));
        let selected = edges.get(*idx % edges.len()).cloned();
        *idx = idx.saturating_add(1);
        selected
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
    use tempfile::tempdir;

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
            tokens: Vec::new(),
        });

        let sessions = service.list_sessions().unwrap();
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].state, SessionState::WaitingInput);
    }

    #[test]
    fn hub_service_registers_and_heartbeats() {
        let tmp = tempfile::tempdir().unwrap();
        let service = HubService::new(HubConfig {
            data_dir: tmp.path().join("hub"),
            edges: vec![],
            tokens: Vec::new(),
        });

        let reg = service.register_edge("edge-a".to_string(), "127.0.0.1:9000".to_string());
        assert_eq!(reg.id, "edge-a");
        let before = reg.last_seen;
        let hb = service.heartbeat("edge-a").unwrap();
        assert!(hb.last_seen >= before);
        assert_eq!(service.list_registered_edges().len(), 1);
    }

    #[test]
    fn hub_service_picks_edges_round_robin() {
        let tmp = tempfile::tempdir().unwrap();
        let service = HubService::new(HubConfig {
            data_dir: tmp.path().join("hub"),
            edges: vec![],
            tokens: Vec::new(),
        });

        service.register_edge("edge-a".to_string(), "127.0.0.1:9000".to_string());
        service.register_edge("edge-b".to_string(), "127.0.0.1:9001".to_string());

        let first = service.pick_edge().unwrap().id;
        let second = service.pick_edge().unwrap().id;
        let third = service.pick_edge().unwrap().id;

        assert_ne!(first, second);
        assert_eq!(first, third);
    }

    #[test]
    fn load_config_allows_missing_edges() {
        let tmp = tempdir().unwrap();
        let cfg_path = tmp.path().join("hub.toml");
        fs::write(&cfg_path, "data_dir = \"./.forgemux-hub\"\n").unwrap();
        let cfg = HubConfig::load(&cfg_path).unwrap();
        assert_eq!(cfg.data_dir, PathBuf::from("./.forgemux-hub"));
        assert!(cfg.edges.is_empty());
        assert!(cfg.tokens.is_empty());
    }
}
