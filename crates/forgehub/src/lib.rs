use anyhow::Context;
use chrono::{DateTime, Utc};
use forgemux_core::{
    Decision, DecisionAction, DecisionResolution, ReplayEvent, RiskLevel, SessionHubMeta,
    SessionRecord, SessionStore, TestsStatus, Workspace, WorkspaceRepo, sort_sessions,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use tokio::time::{Duration, interval};
use tracing::{debug, instrument, warn};

mod db;
mod risk;
use db::{
    decision_count, ensure_workspace, get_decision, get_workspace, init_db, insert_decision,
    insert_replay_event, list_decisions, list_replay_events, list_workspaces, log_budget_action,
    mark_edge_sessions_unreachable, resolve_decision, seed_workspaces, upsert_session_cache,
};
use risk::compute_risk;

pub use db::DecisionStatus;

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
    #[serde(default)]
    pub organization: Option<OrganizationSeed>,
    #[serde(default)]
    pub workspaces: Vec<WorkspaceSeed>,
}

impl HubConfig {
    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let data = fs::read_to_string(path)?;
        let cfg = toml::from_str(&data)?;
        Ok(cfg)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganizationSeed {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSeed {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub org_id: Option<String>,
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default)]
    pub attention_budget_total: Option<u32>,
    #[serde(default)]
    pub repos: Vec<WorkspaceRepo>,
    #[serde(default)]
    pub members: Vec<String>,
}

#[derive(Debug, Clone)]
struct WorkspaceRoots {
    id: String,
    roots: Vec<PathBuf>,
}

pub struct HubService {
    config: HubConfig,
    registry: Arc<Mutex<HashMap<String, EdgeRegistration>>>,
    round_robin: Arc<Mutex<usize>>,
    pairing_tokens: Arc<Mutex<HashMap<String, PairingToken>>>,
    issued_tokens: Arc<Mutex<HashMap<String, DateTime<Utc>>>>,
    decision_tx: broadcast::Sender<DecisionEvent>,
    decision_counter: Arc<Mutex<u64>>,
    edge_failures: Arc<Mutex<HashMap<String, u8>>>,
    workspace_roots: Vec<WorkspaceRoots>,
    default_workspace_id: String,
    #[allow(dead_code)]
    db: SqlitePool,
}

impl HubService {
    #[instrument(skip(config))]
    pub async fn new(config: HubConfig) -> anyhow::Result<Self> {
        let db = init_db(&config.data_dir).await?;
        seed_workspaces(&db, &config).await?;
        let (decision_tx, _) = broadcast::channel(100);
        let counter = decision_count(&db).await?;
        let (workspace_roots, default_workspace_id) = workspace_roots_from_config(&config);
        Ok(Self {
            config,
            registry: Arc::new(Mutex::new(HashMap::new())),
            round_robin: Arc::new(Mutex::new(0)),
            pairing_tokens: Arc::new(Mutex::new(HashMap::new())),
            issued_tokens: Arc::new(Mutex::new(HashMap::new())),
            decision_tx,
            decision_counter: Arc::new(Mutex::new(counter)),
            edge_failures: Arc::new(Mutex::new(HashMap::new())),
            workspace_roots,
            default_workspace_id,
            db,
        })
    }

    pub fn list_edges(&self) -> Vec<HubEdge> {
        self.config.edges.clone()
    }

    #[instrument(skip(self))]
    pub async fn list_workspaces(&self) -> anyhow::Result<Vec<Workspace>> {
        list_workspaces(&self.db).await
    }

    #[instrument(skip(self))]
    pub async fn get_workspace(&self, id: &str) -> anyhow::Result<Option<Workspace>> {
        get_workspace(&self.db, id).await
    }

    pub fn tokens_required(&self) -> bool {
        !self.config.tokens.is_empty()
    }

    pub fn is_token_valid(&self, token: &str) -> bool {
        if self.config.tokens.iter().any(|t| t == token) {
            return true;
        }
        self.cleanup_expired_tokens();
        self.issued_tokens.lock().unwrap().get(token).is_some()
    }

    #[instrument(skip(self))]
    pub fn start_pairing(&self, ttl_secs: i64) -> PairingToken {
        let token = uuid::Uuid::new_v4().simple().to_string();
        let expires_at = Utc::now() + chrono::Duration::seconds(ttl_secs.max(30));
        let record = PairingToken {
            token: token.clone(),
            expires_at,
        };
        self.pairing_tokens
            .lock()
            .unwrap()
            .insert(token.clone(), record.clone());
        record
    }

    #[instrument(skip(self))]
    pub fn exchange_pairing(&self, token: &str, ttl_secs: i64) -> Option<IssuedToken> {
        let mut pairing = self.pairing_tokens.lock().unwrap();
        let record = pairing.remove(token)?;
        if record.expires_at < Utc::now() {
            return None;
        }
        let access = uuid::Uuid::new_v4().simple().to_string();
        let expires_at = Utc::now() + chrono::Duration::seconds(ttl_secs.max(60));
        self.issued_tokens
            .lock()
            .unwrap()
            .insert(access.clone(), expires_at);
        Some(IssuedToken {
            token: access,
            expires_at,
        })
    }

    fn cleanup_expired_tokens(&self) {
        let now = Utc::now();
        self.issued_tokens
            .lock()
            .unwrap()
            .retain(|_, expires| *expires > now);
        self.pairing_tokens
            .lock()
            .unwrap()
            .retain(|_, record| record.expires_at > now);
    }

    #[instrument(skip(self))]
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

    #[instrument(skip(self))]
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

    pub fn workspace_for_repo(&self, repo_root: &Path) -> String {
        let mut best: Option<(String, usize)> = None;
        for workspace in &self.workspace_roots {
            for root in &workspace.roots {
                if repo_root.starts_with(root) {
                    let specificity = root.components().count();
                    if best.as_ref().is_none_or(|(_, s)| specificity > *s) {
                        best = Some((workspace.id.clone(), specificity));
                    }
                }
            }
        }
        best.map(|(id, _)| id)
            .unwrap_or_else(|| self.default_workspace_id.clone())
    }

    pub fn filter_sessions_by_workspace(
        &self,
        sessions: Vec<SessionRecord>,
        workspace_id: &str,
    ) -> Vec<SessionRecord> {
        sessions
            .into_iter()
            .filter(|session| self.workspace_for_repo(&session.repo_root) == workspace_id)
            .collect()
    }

    #[instrument(skip(self))]
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

    #[instrument(skip(self))]
    pub fn resolve_ws_url(&self, edge_id: Option<&str>) -> Option<String> {
        // Check static config edges first
        if let Some(id) = edge_id {
            let url = self
                .config
                .edges
                .iter()
                .find(|edge| edge.id == id)
                .and_then(|edge| edge.ws_url.clone());
            if url.is_some() {
                return url;
            }
        }
        if self.config.edges.len() == 1 && self.config.edges[0].ws_url.is_some() {
            return self.config.edges[0].ws_url.clone();
        }
        // Fall back to dynamically registered edges
        let guard = self.registry.lock().unwrap();
        if let Some(id) = edge_id
            && let Some(edge) = guard.get(id)
        {
            return Some(normalize_ws_addr(&edge.addr));
        }
        if guard.len() == 1 {
            return guard
                .values()
                .next()
                .map(|edge| normalize_ws_addr(&edge.addr));
        }
        None
    }

    #[instrument(skip(self))]
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

    #[instrument(skip(self))]
    pub async fn poll_edges(self: Arc<Self>) {
        let client = reqwest::Client::new();
        let mut ticker = interval(Duration::from_secs(3));
        loop {
            ticker.tick().await;
            let edges = self.list_registered_edges();
            if edges.is_empty() {
                continue;
            }
            let pending = self
                .list_decisions("default", None, Some(DecisionStatus::Pending))
                .await
                .unwrap_or_default();
            for edge in edges {
                let url = format!("{}/sessions", normalize_http_addr(&edge.addr));
                match client.get(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        match resp.json::<Vec<SessionRecord>>().await {
                            Ok(sessions) => {
                                self.mark_edge_success(&edge.id);
                                self.cache_edge_sessions(&edge.id, sessions, &pending).await;
                            }
                            Err(_) => {
                                warn!(edge_id = %edge.id, "edge poll failed (parse)");
                                self.mark_edge_failure(&edge.id).await;
                            }
                        }
                    }
                    _ => {
                        warn!(edge_id = %edge.id, "edge poll failed (http)");
                        self.mark_edge_failure(&edge.id).await;
                    }
                }
            }
        }
    }

    async fn cache_edge_sessions(
        &self,
        edge_id: &str,
        sessions: Vec<SessionRecord>,
        pending: &[Decision],
    ) {
        for session in sessions {
            let goal = session
                .goal
                .clone()
                .unwrap_or_else(|| "(no goal set)".to_string());
            let mut meta = SessionHubMeta {
                workspace_id: "default".to_string(),
                goal,
                risk: RiskLevel::Green,
                context_pct: 0,
                touched_repos: vec![session.repo_root.display().to_string()],
                pending_decisions: pending.len() as u32,
                tests_status: TestsStatus::None,
                tokens_total: "0".to_string(),
                estimated_cost_usd: 0.0,
                lines_added: 0,
                lines_removed: 0,
                commits: 0,
            };
            meta.risk = compute_risk(&meta, pending, session.state.clone(), Utc::now());
            let _ = upsert_session_cache(
                &self.db,
                session.id.as_ref(),
                &meta.workspace_id,
                edge_id,
                &meta,
                session.state,
            )
            .await;
        }
    }

    async fn mark_edge_failure(&self, edge_id: &str) {
        let should_mark = {
            let mut failures = self.edge_failures.lock().unwrap();
            let count = failures.entry(edge_id.to_string()).or_insert(0);
            *count = count.saturating_add(1);
            *count >= 2
        };
        if should_mark {
            let _ = mark_edge_sessions_unreachable(&self.db, edge_id).await;
        }
    }

    fn mark_edge_success(&self, edge_id: &str) {
        let mut failures = self.edge_failures.lock().unwrap();
        failures.insert(edge_id.to_string(), 0);
        debug!(edge_id, "edge poll success");
    }

    pub fn subscribe_decisions(&self) -> broadcast::Receiver<DecisionEvent> {
        self.decision_tx.subscribe()
    }

    #[instrument(skip(self))]
    pub async fn list_decisions(
        &self,
        workspace_id: &str,
        repo_id: Option<&str>,
        status: Option<DecisionStatus>,
    ) -> anyhow::Result<Vec<Decision>> {
        list_decisions(&self.db, workspace_id, repo_id, status).await
    }

    #[instrument(skip(self))]
    pub async fn get_decision(&self, id: &str) -> anyhow::Result<Option<Decision>> {
        get_decision(&self.db, id).await
    }

    #[instrument(skip(self))]
    pub async fn create_decision(&self, mut decision: Decision) -> anyhow::Result<Decision> {
        if decision.id.is_empty() {
            decision.id = self.next_decision_id();
        }
        decision.created_at = Utc::now();
        decision.resolved_at = None;
        decision.resolution = None;
        ensure_workspace(&self.db, &decision.workspace_id).await?;
        insert_decision(&self.db, &decision).await?;
        let _ = self
            .decision_tx
            .send(DecisionEvent::Created(Box::new(decision.clone())));
        Ok(decision)
    }

    #[instrument(skip(self))]
    pub async fn resolve_decision(
        &self,
        decision_id: &str,
        action: DecisionAction,
        reviewer: &str,
        comment: Option<String>,
    ) -> anyhow::Result<()> {
        let resolution = DecisionResolution {
            action,
            reviewer: reviewer.to_string(),
            comment,
            resolved_at: Utc::now(),
        };
        resolve_decision(&self.db, decision_id, &resolution).await?;
        if let Some(decision) = get_decision(&self.db, decision_id).await? {
            log_budget_action(
                &self.db,
                &decision.workspace_id,
                &decision.id,
                reviewer,
                action,
            )
            .await?;
        }
        let _ = self.decision_tx.send(DecisionEvent::Resolved {
            decision_id: decision_id.to_string(),
            action,
        });
        Ok(())
    }

    #[instrument(skip(self))]
    pub async fn record_replay_event(&self, event: ReplayEvent) -> anyhow::Result<()> {
        insert_replay_event(&self.db, &event).await
    }

    #[instrument(skip(self))]
    pub async fn replay_timeline(
        &self,
        session_id: &str,
        after: Option<u64>,
        limit: u32,
    ) -> anyhow::Result<(Vec<ReplayEvent>, Option<u64>)> {
        let events = list_replay_events(&self.db, session_id, after, limit).await?;
        let next_cursor = if events.len() == limit as usize {
            events.last().map(|event| event.id)
        } else {
            None
        };
        Ok((events, next_cursor))
    }

    fn next_decision_id(&self) -> String {
        let mut guard = self.decision_counter.lock().unwrap();
        *guard += 1;
        format!("D-{:04}", *guard)
    }
}

fn workspace_roots_from_config(config: &HubConfig) -> (Vec<WorkspaceRoots>, String) {
    let mut roots = Vec::new();
    for workspace in &config.workspaces {
        let repo_roots = workspace
            .repos
            .iter()
            .filter_map(|repo| repo.root.as_ref())
            .map(PathBuf::from)
            .collect::<Vec<_>>();
        roots.push(WorkspaceRoots {
            id: workspace.id.clone(),
            roots: repo_roots,
        });
    }
    if roots.is_empty() {
        roots.push(WorkspaceRoots {
            id: "default".to_string(),
            roots: Vec::new(),
        });
    }
    let default_id = roots
        .iter()
        .find(|workspace| workspace.id == "default")
        .map(|workspace| workspace.id.clone())
        .unwrap_or_else(|| roots[0].id.clone());
    (roots, default_id)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DecisionEvent {
    Created(Box<Decision>),
    Resolved {
        decision_id: String,
        action: DecisionAction,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingToken {
    pub token: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssuedToken {
    pub token: String,
    pub expires_at: DateTime<Utc>,
}

fn normalize_ws_addr(addr: &str) -> String {
    if addr.starts_with("ws://") || addr.starts_with("wss://") {
        addr.to_string()
    } else if let Some(rest) = addr.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = addr.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        format!("ws://{addr}")
    }
}

fn normalize_http_addr(addr: &str) -> String {
    if addr.starts_with("http://") || addr.starts_with("https://") {
        addr.to_string()
    } else {
        format!("http://{addr}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use forgemux_core::{AgentType, SessionRecord, SessionState};
    use tempfile::tempdir;

    #[tokio::test]
    async fn hub_service_aggregates_sessions() {
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
            organization: None,
            workspaces: Vec::new(),
        })
        .await
        .unwrap();

        let sessions = service.list_sessions().unwrap();
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].state, SessionState::WaitingInput);
    }

    #[tokio::test]
    async fn hub_service_registers_and_heartbeats() {
        let tmp = tempfile::tempdir().unwrap();
        let service = HubService::new(HubConfig {
            data_dir: tmp.path().join("hub"),
            edges: vec![],
            tokens: Vec::new(),
            organization: None,
            workspaces: Vec::new(),
        })
        .await
        .unwrap();

        let reg = service.register_edge("edge-a".to_string(), "127.0.0.1:9000".to_string());
        assert_eq!(reg.id, "edge-a");
        let before = reg.last_seen;
        let hb = service.heartbeat("edge-a").unwrap();
        assert!(hb.last_seen >= before);
        assert_eq!(service.list_registered_edges().len(), 1);
    }

    #[tokio::test]
    async fn hub_service_picks_edges_round_robin() {
        let tmp = tempfile::tempdir().unwrap();
        let service = HubService::new(HubConfig {
            data_dir: tmp.path().join("hub"),
            edges: vec![],
            tokens: Vec::new(),
            organization: None,
            workspaces: Vec::new(),
        })
        .await
        .unwrap();

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

    #[tokio::test]
    async fn pairing_tokens_exchange_for_access_token() {
        let tmp = tempdir().unwrap();
        let service = HubService::new(HubConfig {
            data_dir: tmp.path().join("hub"),
            edges: vec![],
            tokens: Vec::new(),
            organization: None,
            workspaces: Vec::new(),
        })
        .await
        .unwrap();

        let pairing = service.start_pairing(60);
        let issued = service.exchange_pairing(&pairing.token, 300).unwrap();
        assert!(service.is_token_valid(&issued.token));
    }
}
