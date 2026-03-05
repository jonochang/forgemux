use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use cucumber::{World, given, then, when};
use forgehub::{HubConfig, HubService, OrganizationSeed, WorkspaceSeed};
use forgemux_core::{AgentType, SessionRecord, Workspace, WorkspaceRepo};
use futures_util::future::join_all;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

#[derive(Default, World)]
struct HubWorld {
    org: Option<OrganizationSeed>,
    workspaces: HashMap<String, WorkspaceSeed>,
    service: Option<Arc<HubService>>,
    last_workspaces: Vec<forgemux_core::Workspace>,
    last_workspace: Option<forgemux_core::Workspace>,
    resolved_workspace_id: Option<String>,
    selected_workspace_id: Option<String>,
    sessions: Vec<SessionRecord>,
    last_sessions: Vec<SessionRecord>,
    last_http_workspaces: Vec<Workspace>,
    last_http_sessions: Vec<SessionRecord>,
    last_start_payload: Option<serde_json::Value>,
    hub_base_url: Option<String>,
    edge_addr: Option<String>,
    edge_sessions: Vec<SessionRecord>,
    start_capture: Option<Arc<Mutex<Option<serde_json::Value>>>>,
    tempdir: Option<TempDir>,
}

impl std::fmt::Debug for HubWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HubWorld")
            .field("org", &self.org)
            .field("workspaces", &self.workspaces.keys().collect::<Vec<_>>())
            .field("last_workspaces", &self.last_workspaces)
            .field("last_workspace", &self.last_workspace)
            .field("resolved_workspace_id", &self.resolved_workspace_id)
            .field("selected_workspace_id", &self.selected_workspace_id)
            .field("sessions_len", &self.sessions.len())
            .field("last_sessions_len", &self.last_sessions.len())
            .field("last_http_workspaces_len", &self.last_http_workspaces.len())
            .field("last_http_sessions_len", &self.last_http_sessions.len())
            .field("last_start_payload", &self.last_start_payload)
            .field("hub_base_url", &self.hub_base_url)
            .field("edge_addr", &self.edge_addr)
            .finish()
    }
}

impl HubWorld {
    fn ensure_workspace_seed(&mut self, id: &str) -> &mut WorkspaceSeed {
        self.workspaces
            .entry(id.to_string())
            .or_insert_with(|| WorkspaceSeed {
                id: id.to_string(),
                name: id.to_string(),
                org_id: None,
                timezone: None,
                attention_budget_total: None,
                repos: Vec::new(),
                members: Vec::new(),
            })
    }

    async fn ensure_service(&mut self) -> anyhow::Result<()> {
        if self.service.is_some() {
            return Ok(());
        }
        let data_dir = tempfile::tempdir()?;
        let config = HubConfig {
            data_dir: data_dir.path().join("hub"),
            edges: Vec::new(),
            tokens: Vec::new(),
            organization: self.org.clone(),
            workspaces: self.workspaces.values().cloned().collect(),
        };
        self.tempdir = Some(data_dir);
        self.service = Some(Arc::new(HubService::new(config).await?));
        Ok(())
    }

    async fn ensure_hub_server(&mut self) -> anyhow::Result<()> {
        if self.hub_base_url.is_some() {
            return Ok(());
        }
        self.ensure_service().await?;
        let service = Arc::clone(self.service.as_ref().unwrap());
        let app = Router::new()
            .route("/edges/register", post(register_edge_http))
            .route("/workspaces", get(list_workspaces_http))
            .route("/workspaces/:id", get(get_workspace_http))
            .route(
                "/sessions",
                get(list_sessions_http).post(start_session_http),
            )
            .with_state(service);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        self.hub_base_url = Some(format!("http://{}", addr));
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct SessionsQuery {
    workspace_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RegisterEdge {
    id: String,
    addr: String,
}

async fn register_edge_http(
    State(service): State<Arc<HubService>>,
    Json(payload): Json<RegisterEdge>,
) -> (StatusCode, Json<serde_json::Value>) {
    let reg = service.register_edge(payload.id, payload.addr);
    (
        StatusCode::OK,
        Json(serde_json::json!({ "id": reg.id, "addr": reg.addr })),
    )
}

async fn list_workspaces_http(
    State(service): State<Arc<HubService>>,
) -> Result<Json<Vec<Workspace>>, StatusCode> {
    match service.list_workspaces().await {
        Ok(workspaces) => Ok(Json(workspaces)),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn get_workspace_http(
    State(service): State<Arc<HubService>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Workspace>, StatusCode> {
    match service.get_workspace(&id).await {
        Ok(Some(workspace)) => Ok(Json(workspace)),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn list_sessions_http(
    State(service): State<Arc<HubService>>,
    Query(query): Query<SessionsQuery>,
) -> Result<Json<Vec<SessionRecord>>, StatusCode> {
    let mut sessions = fetch_sessions_http(&service).await;
    if let Some(workspace_id) = query.workspace_id.as_deref() {
        sessions = service.filter_sessions_by_workspace(sessions, workspace_id);
    }
    Ok(Json(sessions))
}

async fn start_session_http(
    State(service): State<Arc<HubService>>,
    Json(mut payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let edge_id = payload
        .get("edge_id")
        .and_then(|value| value.as_str())
        .map(|value| value.to_string());
    if let Some(obj) = payload.as_object_mut() {
        obj.remove("edge_id");
    }
    let edge = if let Some(edge_id) = edge_id {
        service
            .list_registered_edges()
            .into_iter()
            .find(|edge| edge.id == edge_id)
    } else {
        service.pick_edge()
    };
    let edge = edge.ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let url = format!("{}/sessions/start", normalize_http_addr(&edge.addr));
    let resp = reqwest::Client::new()
        .post(url)
        .json(&payload)
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    let status = resp.status();
    let body = resp
        .json::<serde_json::Value>()
        .await
        .unwrap_or_else(|_| serde_json::json!({ "error": "invalid response" }));
    if status.is_success() {
        Ok(Json(body))
    } else {
        Err(StatusCode::BAD_GATEWAY)
    }
}

async fn fetch_sessions_http(service: &HubService) -> Vec<SessionRecord> {
    let client = reqwest::Client::new();
    let edges = service.list_registered_edges();
    let futures = edges.into_iter().map(|edge| {
        let url = format!("{}/sessions", normalize_http_addr(&edge.addr));
        let client = client.clone();
        async move { client.get(url).send().await }
    });
    let mut sessions = Vec::new();
    for response in join_all(futures).await.into_iter().flatten() {
        if response.status().is_success()
            && let Ok(mut edge_sessions) = response.json::<Vec<SessionRecord>>().await
        {
            sessions.append(&mut edge_sessions);
        }
    }
    forgemux_core::sort_sessions(sessions)
}

fn normalize_http_addr(addr: &str) -> String {
    if addr.starts_with("http://") || addr.starts_with("https://") {
        addr.to_string()
    } else {
        format!("http://{addr}")
    }
}

#[given(regex = r#"^a hub with no configured workspaces$"#)]
async fn given_no_workspaces(world: &mut HubWorld) {
    world.workspaces.clear();
    world.org = None;
}

#[given(
    regex = r#"^a hub with workspace "([^"]+)" named "([^"]+)" and repo "([^"]+)" labeled "([^"]+)" rooted at "([^"]+)"$"#
)]
async fn given_workspace_with_repo(
    world: &mut HubWorld,
    workspace_id: String,
    workspace_name: String,
    repo_id: String,
    repo_label: String,
    repo_root: String,
) {
    let workspace = world.ensure_workspace_seed(&workspace_id);
    workspace.name = workspace_name;
    workspace.repos.push(WorkspaceRepo {
        id: repo_id,
        label: repo_label,
        icon: "hammer".to_string(),
        color: "#111111".to_string(),
        root: Some(repo_root),
    });
}

#[given(regex = r#"^a hub with workspace root "([^"]+)" at "([^"]+)"$"#)]
async fn given_workspace_root(world: &mut HubWorld, workspace_id: String, root: String) {
    let workspace = world.ensure_workspace_seed(&workspace_id);
    workspace.repos.push(WorkspaceRepo {
        id: format!("{workspace_id}-repo"),
        label: format!("{workspace_id}-repo"),
        icon: "hammer".to_string(),
        color: "#111111".to_string(),
        root: Some(root),
    });
}

#[given(regex = r#"^sessions exist at repo roots "([^"]+)" and "([^"]+)"$"#)]
async fn given_sessions_exist(world: &mut HubWorld, repo_a: String, repo_b: String) {
    let session_a = SessionRecord::new(AgentType::Claude, "sonnet", repo_a.into());
    let session_b = SessionRecord::new(AgentType::Claude, "sonnet", repo_b.into());
    world.sessions = vec![session_a, session_b];
}

#[given(
    regex = r#"^a hub server with workspace "([^"]+)" named "([^"]+)" and repo "([^"]+)" labeled "([^"]+)" rooted at "([^"]+)"$"#
)]
async fn given_hub_server_with_workspace(
    world: &mut HubWorld,
    workspace_id: String,
    workspace_name: String,
    repo_id: String,
    repo_label: String,
    repo_root: String,
) -> anyhow::Result<()> {
    given_workspace_with_repo(
        world,
        workspace_id,
        workspace_name,
        repo_id,
        repo_label,
        repo_root,
    )
    .await;
    Ok(())
}

#[given(regex = r#"^an edge server provides sessions for repo roots "([^"]+)" and "([^"]+)"$"#)]
async fn given_edge_server_sessions(
    world: &mut HubWorld,
    repo_a: String,
    repo_b: String,
) -> anyhow::Result<()> {
    let session_a = SessionRecord::new(AgentType::Claude, "sonnet", repo_a.into());
    let session_b = SessionRecord::new(AgentType::Claude, "sonnet", repo_b.into());
    world.edge_sessions = vec![session_a, session_b];

    let sessions = world.edge_sessions.clone();
    let edge_app = Router::new().route(
        "/sessions",
        get(move || {
            let sessions = sessions.clone();
            async move { Json(sessions) }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move {
        axum::serve(listener, edge_app).await.unwrap();
    });
    world.edge_addr = Some(addr.to_string());
    Ok(())
}

#[given(regex = r#"^an edge server accepts session starts$"#)]
async fn given_edge_server_accepts_starts(world: &mut HubWorld) -> anyhow::Result<()> {
    let capture = Arc::new(Mutex::new(None));
    let capture_clone = Arc::clone(&capture);
    let edge_app = Router::new().route(
        "/sessions/start",
        post(move |Json(payload): Json<serde_json::Value>| async move {
            *capture_clone.lock().unwrap() = Some(payload);
            Json(serde_json::json!({ "session_id": "S-EDGE" }))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move {
        axum::serve(listener, edge_app).await.unwrap();
    });
    world.edge_addr = Some(addr.to_string());
    world.start_capture = Some(capture);
    Ok(())
}

#[when(regex = r#"^I list workspaces$"#)]
async fn when_list_workspaces(world: &mut HubWorld) -> anyhow::Result<()> {
    world.ensure_service().await?;
    let service = world.service.as_ref().unwrap();
    world.last_workspaces = service.list_workspaces().await?;
    Ok(())
}

#[when(regex = r#"^I get workspace "([^"]+)"$"#)]
async fn when_get_workspace(world: &mut HubWorld, workspace_id: String) -> anyhow::Result<()> {
    world.ensure_service().await?;
    let service = world.service.as_ref().unwrap();
    world.last_workspace = service.get_workspace(&workspace_id).await?;
    Ok(())
}

#[when(regex = r#"^I select workspace "([^"]+)"$"#)]
async fn when_select_workspace(world: &mut HubWorld, workspace_id: String) {
    world.selected_workspace_id = Some(workspace_id);
}

#[when(regex = r#"^I register the edge with the hub$"#)]
async fn when_register_edge(world: &mut HubWorld) -> anyhow::Result<()> {
    world.ensure_hub_server().await?;
    let hub = world.hub_base_url.as_ref().unwrap();
    let edge_addr = world.edge_addr.as_ref().unwrap();
    let payload = serde_json::json!({
        "id": "edge-01",
        "addr": edge_addr,
    });
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{hub}/edges/register"))
        .json(&payload)
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!("edge register failed: {}", resp.status());
    }
    Ok(())
}

#[when(regex = r#"^I start a session named "([^"]+)" with model "([^"]+)"$"#)]
async fn when_start_session_named_model(
    world: &mut HubWorld,
    name: String,
    model: String,
) -> anyhow::Result<()> {
    world.ensure_hub_server().await?;
    let hub = world.hub_base_url.as_ref().unwrap();
    let payload = serde_json::json!({
        "edge_id": "edge-01",
        "agent": "claude",
        "model": model,
        "name": name,
        "repo": "/repos/a",
        "worktree": false,
        "branch": null
    });
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{hub}/sessions"))
        .json(&payload)
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!("start session failed: {}", resp.status());
    }
    Ok(())
}

#[when(regex = r#"^I request workspaces via HTTP$"#)]
async fn when_request_workspaces_http(world: &mut HubWorld) -> anyhow::Result<()> {
    world.ensure_hub_server().await?;
    let hub = world.hub_base_url.as_ref().unwrap();
    let client = reqwest::Client::new();
    let resp = client.get(format!("{hub}/workspaces")).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("workspaces http failed: {}", resp.status());
    }
    world.last_http_workspaces = resp.json::<Vec<Workspace>>().await?;
    Ok(())
}

#[when(regex = r#"^I resolve workspace for repo root "([^"]+)"$"#)]
async fn when_resolve_workspace(world: &mut HubWorld, repo_root: String) -> anyhow::Result<()> {
    world.ensure_service().await?;
    let service = world.service.as_ref().unwrap();
    world.resolved_workspace_id = Some(service.workspace_for_repo(Path::new(&repo_root)));
    Ok(())
}

#[when(regex = r#"^I request sessions for workspace "([^"]+)" via HTTP$"#)]
async fn when_request_sessions_for_workspace_http(
    world: &mut HubWorld,
    workspace_id: String,
) -> anyhow::Result<()> {
    world.ensure_hub_server().await?;
    let hub = world.hub_base_url.as_ref().unwrap();
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{hub}/sessions"))
        .query(&[("workspace_id", workspace_id)])
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!("sessions http failed: {}", resp.status());
    }
    world.last_http_sessions = resp.json::<Vec<SessionRecord>>().await?;
    Ok(())
}

#[when(regex = r#"^I list sessions for workspace "([^"]+)"$"#)]
async fn when_list_sessions_for_workspace(
    world: &mut HubWorld,
    workspace_id: String,
) -> anyhow::Result<()> {
    world.ensure_service().await?;
    let service = world.service.as_ref().unwrap();
    world.last_sessions =
        service.filter_sessions_by_workspace(world.sessions.clone(), &workspace_id);
    Ok(())
}

#[then(regex = r#"^the workspace list contains "([^"]+)"$"#)]
async fn then_workspace_list_contains(world: &mut HubWorld, workspace_id: String) {
    let found = world
        .last_workspaces
        .iter()
        .any(|workspace| workspace.id == workspace_id);
    assert!(found, "expected workspace list to contain {workspace_id}");
}

#[then(regex = r#"^the workspace has repo "([^"]+)" labeled "([^"]+)"$"#)]
async fn then_workspace_has_repo(world: &mut HubWorld, repo_id: String, label: String) {
    let workspace = world
        .last_workspace
        .as_ref()
        .expect("workspace should be loaded");
    let repo = workspace
        .repos
        .iter()
        .find(|repo| repo.id == repo_id)
        .expect("repo should exist");
    assert_eq!(repo.label, label);
}

#[then(regex = r#"^the HTTP workspace list contains "([^"]+)" and "([^"]+)"$"#)]
async fn then_http_workspaces_contain(world: &mut HubWorld, a: String, b: String) {
    let ids: Vec<_> = world
        .last_http_workspaces
        .iter()
        .map(|ws| ws.id.as_str())
        .collect();
    assert!(ids.contains(&a.as_str()), "expected workspace {a}");
    assert!(ids.contains(&b.as_str()), "expected workspace {b}");
}

#[then(regex = r#"^the active workspace is "([^"]+)"$"#)]
async fn then_active_workspace_is(world: &mut HubWorld, expected: String) {
    assert_eq!(
        world.selected_workspace_id.as_deref(),
        Some(expected.as_str())
    );
}

#[then(regex = r#"^the resolved workspace id is "([^"]+)"$"#)]
async fn then_resolved_workspace_is(world: &mut HubWorld, expected: String) {
    assert_eq!(
        world.resolved_workspace_id.as_deref(),
        Some(expected.as_str())
    );
}

#[then(regex = r#"^the session list contains session for repo root "([^"]+)"$"#)]
async fn then_sessions_contain_repo(world: &mut HubWorld, repo_root: String) {
    let found = world
        .last_sessions
        .iter()
        .any(|session| session.repo_root == Path::new(&repo_root));
    assert!(found, "expected session for {repo_root} to be present");
}

#[then(regex = r#"^the session list excludes session for repo root "([^"]+)"$"#)]
async fn then_sessions_exclude_repo(world: &mut HubWorld, repo_root: String) {
    let found = world
        .last_sessions
        .iter()
        .any(|session| session.repo_root == Path::new(&repo_root));
    assert!(!found, "expected session for {repo_root} to be absent");
}

#[then(regex = r#"^the HTTP session list contains session for repo root "([^"]+)"$"#)]
async fn then_http_sessions_contain_repo(world: &mut HubWorld, repo_root: String) {
    let found = world
        .last_http_sessions
        .iter()
        .any(|session| session.repo_root == Path::new(&repo_root));
    assert!(found, "expected HTTP session for {repo_root} to be present");
}

#[then(regex = r#"^the HTTP session list excludes session for repo root "([^"]+)"$"#)]
async fn then_http_sessions_exclude_repo(world: &mut HubWorld, repo_root: String) {
    let found = world
        .last_http_sessions
        .iter()
        .any(|session| session.repo_root == Path::new(&repo_root));
    assert!(!found, "expected HTTP session for {repo_root} to be absent");
}

#[then(regex = r#"^the edge receives a start request with model "([^"]+)" and name "([^"]+)"$"#)]
async fn then_edge_receives_start_request(world: &mut HubWorld, model: String, name: String) {
    let capture = world.start_capture.as_ref().expect("start capture missing");
    let payload = capture
        .lock()
        .unwrap()
        .clone()
        .expect("no payload captured");
    let got_model = payload.get("model").and_then(|v| v.as_str()).unwrap_or("");
    let got_name = payload.get("name").and_then(|v| v.as_str()).unwrap_or("");
    assert_eq!(got_model, model);
    assert_eq!(got_name, name);
}

#[tokio::main]
async fn main() {
    HubWorld::run("./tests/features").await;
}
