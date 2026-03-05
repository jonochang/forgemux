use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::{Element, Page};
use cucumber::{World, given, then, when};
use forgehub::{HubConfig, HubService, OrganizationSeed, WorkspaceSeed};
use forgemux_core::{
    AgentType, Decision, DecisionAction, DecisionContext, SessionRecord, Severity, Workspace,
    WorkspaceRepo,
};
use futures_util::{StreamExt, future::join_all};
use include_dir::{Dir, include_dir};
use mime_guess::MimeGuess;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tempfile::TempDir;
use tokio::time::{Duration, sleep};

const DASHBOARD_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/../../dashboard");

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
    last_input_payload: Option<serde_json::Value>,
    last_decisions: Vec<Decision>,
    last_decision_id: Option<String>,
    hub_base_url: Option<String>,
    edge_addr: Option<String>,
    edge_sessions: Vec<SessionRecord>,
    start_capture: Option<Arc<Mutex<Option<serde_json::Value>>>>,
    input_capture: Option<Arc<Mutex<Option<serde_json::Value>>>>,
    decision_capture: Option<Arc<Mutex<Option<serde_json::Value>>>>,
    dashboard_base_url: Option<String>,
    browser: Option<Browser>,
    page: Option<Page>,
    browser_tempdir: Option<TempDir>,
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
            .field("last_input_payload", &self.last_input_payload)
            .field("last_decision_id", &self.last_decision_id)
            .field("hub_base_url", &self.hub_base_url)
            .field("edge_addr", &self.edge_addr)
            .field("dashboard_base_url", &self.dashboard_base_url)
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
            .route("/edges", get(list_edges_http))
            .route("/edges/:id/config", get(edge_config_http))
            .route(
                "/decisions",
                get(list_decisions_http).post(create_decision_http),
            )
            .route("/decisions/:id/approve", post(decision_approve_http))
            .route("/decisions/:id/deny", post(decision_deny_http))
            .route("/decisions/:id/comment", post(decision_comment_http))
            .route("/workspaces", get(list_workspaces_http))
            .route("/workspaces/:id", get(get_workspace_http))
            .route(
                "/sessions",
                get(list_sessions_http).post(start_session_http),
            )
            .route("/sessions/:id/input", post(session_input_http))
            .route("/", get(dashboard_index_http))
            .route("/index.html", get(dashboard_index_http))
            .fallback(get(dashboard_asset_http))
            .with_state(service);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let base = format!("http://{}", addr);
        self.hub_base_url = Some(base.clone());
        self.dashboard_base_url = Some(base);
        Ok(())
    }

    async fn ensure_browser(&mut self) -> anyhow::Result<()> {
        if self.browser.is_some() && self.page.is_some() {
            return Ok(());
        }
        let browser_dir = tempfile::tempdir()?;
        let config = BrowserConfig::builder()
            .no_sandbox()
            .user_data_dir(browser_dir.path())
            .build()
            .map_err(anyhow::Error::msg)?;
        let (browser, mut handler) = Browser::launch(config).await?;
        tokio::spawn(async move { while handler.next().await.is_some() {} });
        let page = browser.new_page("about:blank").await?;
        self.browser = Some(browser);
        self.page = Some(page);
        self.browser_tempdir = Some(browser_dir);
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

#[derive(Debug, Deserialize)]
struct DecisionsQuery {
    workspace_id: String,
}

#[derive(Debug, Deserialize)]
struct CreateDecisionRequest {
    session_id: String,
    workspace_id: String,
    repo_id: String,
    question: String,
    context: DecisionContext,
    severity: Severity,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    impact_repo_ids: Vec<String>,
    assigned_to: Option<String>,
    agent_goal: String,
}

#[derive(Debug, Deserialize)]
struct DecisionActionRequest {
    reviewer: String,
    comment: Option<String>,
}

async fn list_decisions_http(
    State(service): State<Arc<HubService>>,
    Query(query): Query<DecisionsQuery>,
) -> Result<Json<Vec<Decision>>, StatusCode> {
    match service
        .list_decisions(&query.workspace_id, None, None)
        .await
    {
        Ok(decisions) => Ok(Json(decisions)),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn create_decision_http(
    State(service): State<Arc<HubService>>,
    Json(req): Json<CreateDecisionRequest>,
) -> Result<Json<Decision>, StatusCode> {
    let decision = Decision {
        id: String::new(),
        session_id: req.session_id,
        workspace_id: req.workspace_id,
        repo_id: req.repo_id,
        question: req.question,
        context: req.context,
        severity: req.severity,
        tags: req.tags,
        impact_repo_ids: req.impact_repo_ids,
        assigned_to: req.assigned_to,
        agent_goal: req.agent_goal,
        created_at: chrono::Utc::now(),
        resolved_at: None,
        resolution: None,
    };
    match service.create_decision(decision).await {
        Ok(created) => Ok(Json(created)),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn decision_approve_http(
    State(service): State<Arc<HubService>>,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<DecisionActionRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    decision_action_http(service, id, DecisionAction::Approve, req).await
}

async fn decision_deny_http(
    State(service): State<Arc<HubService>>,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<DecisionActionRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    decision_action_http(service, id, DecisionAction::Deny, req).await
}

async fn decision_comment_http(
    State(service): State<Arc<HubService>>,
    AxumPath(id): AxumPath<String>,
    Json(req): Json<DecisionActionRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if req.comment.as_deref().unwrap_or("").is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    decision_action_http(service, id, DecisionAction::Comment, req).await
}

async fn decision_action_http(
    service: Arc<HubService>,
    decision_id: String,
    action: DecisionAction,
    req: DecisionActionRequest,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let decision = service
        .get_decision(&decision_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    service
        .resolve_decision(&decision_id, action, &req.reviewer, req.comment.clone())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if action != DecisionAction::Comment {
        let ok = forward_decision_response_http(&service, &decision.session_id, action, &req).await;
        if !ok {
            return Err(StatusCode::BAD_GATEWAY);
        }
    }
    Ok(Json(serde_json::json!({
        "decision_id": decision_id,
        "action": format!("{action:?}").to_lowercase()
    })))
}

async fn forward_decision_response_http(
    service: &HubService,
    session_id: &str,
    action: DecisionAction,
    req: &DecisionActionRequest,
) -> bool {
    let payload = serde_json::json!({
        "action": action,
        "reviewer": req.reviewer,
        "comment": req.comment,
    });
    for edge in service.list_registered_edges() {
        let url = format!(
            "{}/sessions/{}/decision-response",
            normalize_http_addr(&edge.addr),
            session_id
        );
        if let Ok(resp) = reqwest::Client::new().post(url).json(&payload).send().await
            && resp.status().is_success()
        {
            return true;
        }
    }
    false
}

async fn session_input_http(
    State(service): State<Arc<HubService>>,
    AxumPath(id): AxumPath<String>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    for edge in service.list_registered_edges() {
        let url = format!("{}/sessions/{}/input", normalize_http_addr(&edge.addr), id);
        let resp = reqwest::Client::new()
            .post(url)
            .json(&payload)
            .send()
            .await
            .map_err(|_| StatusCode::BAD_GATEWAY)?;
        if resp.status().is_success() {
            let body = resp
                .json::<serde_json::Value>()
                .await
                .unwrap_or_else(|_| serde_json::json!({ "ok": true }));
            return Ok(Json(body));
        }
    }
    Err(StatusCode::BAD_GATEWAY)
}
async fn list_edges_http(
    State(service): State<Arc<HubService>>,
) -> Result<Json<Vec<forgehub::EdgeRegistration>>, StatusCode> {
    Ok(Json(service.list_registered_edges()))
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

async fn edge_config_http(
    State(service): State<Arc<HubService>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let edge = service
        .list_registered_edges()
        .into_iter()
        .find(|edge| edge.id == id)
        .ok_or(StatusCode::NOT_FOUND)?;
    let url = format!("{}/config", normalize_http_addr(&edge.addr));
    let resp = reqwest::Client::new()
        .get(url)
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

async fn wait_for_element(page: &Page, selector: &str) -> anyhow::Result<Element> {
    let start = Instant::now();
    loop {
        match page.find_element(selector).await {
            Ok(el) => return Ok(el),
            Err(_) => {
                if start.elapsed() > Duration::from_secs(8) {
                    anyhow::bail!("timed out waiting for selector: {selector}");
                }
                sleep(Duration::from_millis(50)).await;
            }
        }
    }
}

async fn select_option(page: &Page, selector: &str, value: &str) -> anyhow::Result<()> {
    let selector_json = serde_json::to_string(selector)?;
    let value_json = serde_json::to_string(value)?;
    let script = format!(
        r#"(function() {{
          const el = document.querySelector({selector_json});
          if (!el) return "missing";
          el.value = {value_json};
          el.dispatchEvent(new Event("change", {{ bubbles: true }}));
          return el.value;
        }})()"#
    );
    let result = page.evaluate(script).await?.into_value::<String>()?;
    if result == "missing" {
        anyhow::bail!("missing select element: {selector}");
    }
    Ok(())
}

async fn set_input_value(page: &Page, selector: &str, value: &str) -> anyhow::Result<()> {
    let selector_json = serde_json::to_string(selector)?;
    let value_json = serde_json::to_string(value)?;
    let script = format!(
        r#"(function() {{
          const el = document.querySelector({selector_json});
          if (!el) return "missing";
          el.value = {value_json};
          el.dispatchEvent(new Event("input", {{ bubbles: true }}));
          el.dispatchEvent(new Event("change", {{ bubbles: true }}));
          return el.value;
        }})()"#
    );
    let result = page.evaluate(script).await?.into_value::<String>()?;
    if result == "missing" {
        anyhow::bail!("missing input element: {selector}");
    }
    Ok(())
}

fn dashboard_bytes(path: &str) -> &'static [u8] {
    DASHBOARD_DIR
        .get_file(path)
        .map(|file| file.contents())
        .unwrap_or_else(|| b"")
}

async fn dashboard_index_http() -> impl IntoResponse {
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    (
        axum::http::StatusCode::OK,
        headers,
        dashboard_bytes("index.html"),
    )
}

async fn dashboard_asset_http(uri: Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');
    if path.is_empty() || path == "index.html" {
        return dashboard_index_http().await.into_response();
    }
    if let Some(body) = DASHBOARD_DIR.get_file(path).map(|file| file.contents()) {
        let mut headers = HeaderMap::new();
        let mime = MimeGuess::from_path(path).first_or_octet_stream();
        if let Ok(value) = HeaderValue::from_str(mime.as_ref()) {
            headers.insert(axum::http::header::CONTENT_TYPE, value);
        }
        return (axum::http::StatusCode::OK, headers, body).into_response();
    }
    axum::http::StatusCode::NOT_FOUND.into_response()
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

#[given(regex = r#"^an edge server advertises models and accepts session starts$"#)]
async fn given_edge_server_advertises_models(world: &mut HubWorld) -> anyhow::Result<()> {
    let capture = Arc::new(Mutex::new(None));
    let capture_clone = Arc::clone(&capture);
    let edge_app = Router::new()
        .route(
            "/sessions/start",
            post(move |Json(payload): Json<serde_json::Value>| async move {
                *capture_clone.lock().unwrap() = Some(payload);
                Json(serde_json::json!({ "session_id": "S-EDGE" }))
            }),
        )
        .route(
            "/config",
            get(|| async {
                Json(serde_json::json!({
                    "default_repo": "/repos/alpha",
                    "models_by_agent": {
                        "claude": ["haiku", "sonnet", "opus"],
                        "codex": ["gpt-5.3-codex", "gpt-5.2-codex"]
                    }
                }))
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

#[given(regex = r#"^an edge server accepts session inputs for session "([^"]+)"$"#)]
async fn given_edge_server_accepts_inputs(
    world: &mut HubWorld,
    session_id: String,
) -> anyhow::Result<()> {
    let capture = Arc::new(Mutex::new(None));
    let capture_clone = Arc::clone(&capture);
    let path = format!("/sessions/{}/input", session_id);
    let edge_app = Router::new().route(
        &path,
        post(move |Json(payload): Json<serde_json::Value>| async move {
            *capture_clone.lock().unwrap() = Some(payload);
            Json(serde_json::json!({ "ok": true }))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move {
        axum::serve(listener, edge_app).await.unwrap();
    });
    world.edge_addr = Some(addr.to_string());
    world.input_capture = Some(capture);
    Ok(())
}

#[given(regex = r#"^an edge server accepts decision responses for session "([^"]+)"$"#)]
async fn given_edge_server_accepts_decisions(
    world: &mut HubWorld,
    session_id: String,
) -> anyhow::Result<()> {
    let capture = Arc::new(Mutex::new(None));
    let capture_clone = Arc::clone(&capture);
    let path = format!("/sessions/{}/decision-response", session_id);
    let edge_app = Router::new().route(
        &path,
        post(move |Json(payload): Json<serde_json::Value>| async move {
            *capture_clone.lock().unwrap() = Some(payload);
            Json(serde_json::json!({ "ok": true }))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move {
        axum::serve(listener, edge_app).await.unwrap();
    });
    world.edge_addr = Some(addr.to_string());
    world.decision_capture = Some(capture);
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

#[when(
    regex = r#"^I start a session in repo "([^"]+)" with worktree "([^"]+)" and branch "([^"]+)"$"#
)]
async fn when_start_session_with_worktree(
    world: &mut HubWorld,
    repo: String,
    worktree: String,
    branch: String,
) -> anyhow::Result<()> {
    world.ensure_hub_server().await?;
    let hub = world.hub_base_url.as_ref().unwrap();
    let payload = serde_json::json!({
        "edge_id": "edge-01",
        "agent": "claude",
        "model": "sonnet",
        "name": "Worktree test",
        "repo": repo,
        "worktree": worktree == "true",
        "branch": branch
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

#[when(regex = r#"^I send session input "([^"]+)" for session "([^"]+)"$"#)]
async fn when_send_session_input(
    world: &mut HubWorld,
    input: String,
    session_id: String,
) -> anyhow::Result<()> {
    world.ensure_hub_server().await?;
    let hub = world.hub_base_url.as_ref().unwrap();
    let payload = serde_json::json!({
        "type": "input",
        "input_id": "I-1",
        "data": input,
    });
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{hub}/sessions/{session_id}/input"))
        .json(&payload)
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!("session input failed: {}", resp.status());
    }
    Ok(())
}

#[when(regex = r#"^I create a decision for session "([^"]+)" in workspace "([^"]+)"$"#)]
async fn when_create_decision(
    world: &mut HubWorld,
    session_id: String,
    workspace_id: String,
) -> anyhow::Result<()> {
    world.ensure_hub_server().await?;
    let hub = world.hub_base_url.as_ref().unwrap();
    let payload = serde_json::json!({
        "session_id": session_id,
        "workspace_id": workspace_id,
        "repo_id": "alpha",
        "question": "Ship it?",
        "context": { "type": "log", "text": "Need approval" },
        "severity": "high",
        "tags": [],
        "impact_repo_ids": [],
        "assigned_to": null,
        "agent_goal": "Release"
    });
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{hub}/decisions"))
        .json(&payload)
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!("create decision failed: {}", resp.status());
    }
    let decision = resp.json::<Decision>().await?;
    world.last_decision_id = Some(decision.id);
    Ok(())
}

#[when(regex = r#"^I list decisions for workspace "([^"]+)"$"#)]
async fn when_list_decisions(world: &mut HubWorld, workspace_id: String) -> anyhow::Result<()> {
    world.ensure_hub_server().await?;
    let hub = world.hub_base_url.as_ref().unwrap();
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{hub}/decisions?workspace_id={workspace_id}"))
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!("list decisions failed: {}", resp.status());
    }
    world.last_decisions = resp.json::<Vec<Decision>>().await?;
    Ok(())
}

#[when(regex = r#"^I approve the decision as "([^"]+)"$"#)]
async fn when_approve_decision(world: &mut HubWorld, reviewer: String) -> anyhow::Result<()> {
    world.ensure_hub_server().await?;
    let hub = world.hub_base_url.as_ref().unwrap();
    let decision_id = world
        .last_decision_id
        .as_ref()
        .expect("decision id missing");
    let payload = serde_json::json!({
        "reviewer": reviewer,
        "comment": "ok"
    });
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{hub}/decisions/{decision_id}/approve"))
        .json(&payload)
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!("approve decision failed: {}", resp.status());
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

#[then(
    regex = r#"^the edge receives a start request with worktree "([^"]+)" and branch "([^"]+)"$"#
)]
async fn then_edge_receives_start_request_worktree(
    world: &mut HubWorld,
    worktree: String,
    branch: String,
) -> anyhow::Result<()> {
    let capture = world.start_capture.as_ref().expect("start capture missing");
    let payload = capture
        .lock()
        .unwrap()
        .clone()
        .expect("no start payload captured");
    let got_worktree = payload
        .get("worktree")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let got_branch = payload.get("branch").and_then(|v| v.as_str()).unwrap_or("");
    assert_eq!(got_worktree, worktree == "true");
    assert_eq!(got_branch, branch);
    Ok(())
}

#[then(regex = r#"^the edge receives session input "([^"]+)"$"#)]
async fn then_edge_receives_session_input(
    world: &mut HubWorld,
    input: String,
) -> anyhow::Result<()> {
    let capture = world.input_capture.as_ref().expect("input capture missing");
    let payload = capture
        .lock()
        .unwrap()
        .clone()
        .expect("no input payload captured");
    let got = payload.get("data").and_then(|v| v.as_str()).unwrap_or("");
    assert_eq!(got, input);
    Ok(())
}

#[then(regex = r#"^the decision list contains the new decision$"#)]
async fn then_decision_list_contains_new(world: &mut HubWorld) -> anyhow::Result<()> {
    let id = world
        .last_decision_id
        .as_ref()
        .expect("decision id missing");
    let found = world
        .last_decisions
        .iter()
        .any(|decision| &decision.id == id);
    assert!(found, "decision id not found in list");
    Ok(())
}

#[then(regex = r#"^the edge receives decision response "([^"]+)" by "([^"]+)"$"#)]
async fn then_edge_receives_decision_response(
    world: &mut HubWorld,
    action: String,
    reviewer: String,
) -> anyhow::Result<()> {
    let capture = world
        .decision_capture
        .as_ref()
        .expect("decision capture missing");
    let payload = capture
        .lock()
        .unwrap()
        .clone()
        .expect("no decision payload captured");
    let got_action = payload.get("action").and_then(|v| v.as_str()).unwrap_or("");
    let got_reviewer = payload
        .get("reviewer")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert_eq!(got_action, action);
    assert_eq!(got_reviewer, reviewer);
    Ok(())
}

#[given(regex = r#"^a hub dashboard is running$"#)]
async fn given_hub_dashboard_running(world: &mut HubWorld) -> anyhow::Result<()> {
    world.ensure_hub_server().await?;
    Ok(())
}

#[when(regex = r#"^I open the dashboard$"#)]
async fn when_open_dashboard(world: &mut HubWorld) -> anyhow::Result<()> {
    world.ensure_hub_server().await?;
    world.ensure_browser().await?;
    let base = world.dashboard_base_url.as_ref().unwrap();
    let page = world.page.as_ref().unwrap();
    page.goto(format!("{base}/#attach")).await?;
    wait_for_element(page, "[data-testid='agent-select']").await?;
    Ok(())
}

#[when(regex = r#"^I select agent "([^"]+)"$"#)]
async fn when_select_agent(world: &mut HubWorld, agent: String) -> anyhow::Result<()> {
    let page = world.page.as_ref().unwrap();
    wait_for_element(page, "[data-testid='agent-select']").await?;
    select_option(page, "[data-testid='agent-select']", &agent).await?;
    Ok(())
}

#[when(regex = r#"^I select model "([^"]+)"$"#)]
async fn when_select_model(world: &mut HubWorld, model: String) -> anyhow::Result<()> {
    let page = world.page.as_ref().unwrap();
    wait_for_element(page, "[data-testid='model-select']").await?;
    select_option(page, "[data-testid='model-select']", &model).await?;
    Ok(())
}

#[when(regex = r#"^I enter repo "([^"]+)"$"#)]
async fn when_enter_repo(world: &mut HubWorld, repo: String) -> anyhow::Result<()> {
    let page = world.page.as_ref().unwrap();
    wait_for_element(page, "[data-testid='repo-input']").await?;
    set_input_value(page, "[data-testid='repo-input']", &repo).await?;
    Ok(())
}

#[when(regex = r#"^I start the session$"#)]
async fn when_start_session_ui(world: &mut HubWorld) -> anyhow::Result<()> {
    let page = world.page.as_ref().unwrap();
    let button = wait_for_element(page, "[data-testid='start-session']").await?;
    button.click().await?;
    Ok(())
}

#[then(regex = r#"^the edge receives a start request with agent "([^"]+)" and model "([^"]+)"$"#)]
async fn then_edge_receives_start_request_agent_model(
    world: &mut HubWorld,
    agent: String,
    model: String,
) -> anyhow::Result<()> {
    let capture = world.start_capture.as_ref().expect("start capture missing");
    let start = Instant::now();
    let payload = loop {
        if let Some(payload) = capture.lock().unwrap().clone() {
            break payload;
        }
        if start.elapsed() > Duration::from_secs(5) {
            anyhow::bail!("no start payload captured");
        }
        sleep(Duration::from_millis(50)).await;
    };
    let got_agent = payload.get("agent").and_then(|v| v.as_str()).unwrap_or("");
    let got_model = payload.get("model").and_then(|v| v.as_str()).unwrap_or("");
    assert_eq!(got_agent, agent, "agent mismatch in start payload");
    assert_eq!(got_model, model, "model mismatch in start payload");
    Ok(())
}

#[tokio::main]
async fn main() {
    HubWorld::run("./tests/features").await;
}
