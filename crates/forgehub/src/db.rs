use crate::{HubConfig, OrganizationSeed, WorkspaceSeed};
use anyhow::Context;
use chrono::{DateTime, Datelike, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use forgemux_core::{
    AttentionBudget, Decision, DecisionAction, DecisionContext, DecisionResolution, ReplayEvent,
    ReplayEventType, SessionHubMeta, SessionState, Severity, Workspace, WorkspaceRepo,
    HandoffRecord, HandoffStatus, Role,
};
use sqlx::FromRow;
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::path::Path;

pub async fn init_db(data_dir: &Path) -> anyhow::Result<SqlitePool> {
    std::fs::create_dir_all(data_dir)
        .with_context(|| format!("failed to create hub data dir {}", data_dir.display()))?;
    let db_path = data_dir.join("hub.db");
    let options = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .connect_with(options)
        .await
        .context("failed to open hub database")?;
    sqlx::migrate!()
        .run(&pool)
        .await
        .context("failed to run hub migrations")?;
    Ok(pool)
}

pub async fn seed_workspaces(pool: &SqlitePool, config: &HubConfig) -> anyhow::Result<()> {
    let org = config.organization.clone().unwrap_or(OrganizationSeed {
        id: "org-default".to_string(),
        name: "Default Org".to_string(),
    });
    insert_organization(pool, &org).await?;

    let workspaces = if config.workspaces.is_empty() {
        vec![WorkspaceSeed {
            id: "default".to_string(),
            name: "default".to_string(),
            org_id: Some(org.id.clone()),
            timezone: Some("UTC".to_string()),
            attention_budget_total: Some(12),
            repos: Vec::new(),
            members: Vec::new(),
        }]
    } else {
        config.workspaces.clone()
    };

    for workspace in workspaces {
        insert_workspace(pool, &org, &workspace).await?;
    }
    Ok(())
}

async fn insert_organization(pool: &SqlitePool, org: &OrganizationSeed) -> anyhow::Result<()> {
    sqlx::query("INSERT OR IGNORE INTO organizations (id, name) VALUES (?, ?)")
        .bind(&org.id)
        .bind(&org.name)
        .execute(pool)
        .await?;
    Ok(())
}

async fn insert_workspace(
    pool: &SqlitePool,
    default_org: &OrganizationSeed,
    workspace: &WorkspaceSeed,
) -> anyhow::Result<()> {
    let org_id = workspace.org_id.as_ref().unwrap_or(&default_org.id);
    sqlx::query(
        r#"
        INSERT OR IGNORE INTO workspaces
            (id, org_id, name, timezone, attention_budget_total, repos_json, members_json)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&workspace.id)
    .bind(org_id)
    .bind(&workspace.name)
    .bind(workspace.timezone.as_deref().unwrap_or("UTC"))
    .bind(workspace.attention_budget_total.unwrap_or(12) as i64)
    .bind(serde_json::to_string(&workspace.repos)?)
    .bind(serde_json::to_string(&workspace.members)?)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionStatus {
    Pending,
    Resolved,
}

#[derive(Debug, Clone, FromRow)]
struct DecisionRow {
    id: String,
    session_id: String,
    workspace_id: String,
    repo_id: String,
    question: String,
    context_json: String,
    severity: String,
    tags_json: String,
    impact_repo_ids: String,
    assigned_to: Option<String>,
    agent_goal: String,
    created_at: String,
    resolved_at: Option<String>,
    resolution_json: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
struct WorkspaceRow {
    id: String,
    org_id: String,
    name: String,
    timezone: String,
    attention_budget_total: i64,
    repos_json: String,
    members_json: String,
}

#[derive(Debug, Clone, FromRow)]
struct ReplayEventRow {
    id: i64,
    session_id: String,
    repo_id: Option<String>,
    timestamp: String,
    elapsed: String,
    event_type: String,
    action: String,
    result: Option<String>,
    payload: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
struct HandoffRow {
    id: String,
    role_from: String,
    role_to: String,
    status: String,
    session_id_from: Option<String>,
    artifact_type: String,
    summary: String,
    acceptance_json: String,
    github_owner: String,
    github_repo: String,
    github_issue_number: i64,
    github_pr_number: Option<i64>,
    created_at: String,
    updated_at: String,
    claimed_by: Option<String>,
    completed_by: Option<String>,
}

pub async fn insert_decision(pool: &SqlitePool, decision: &Decision) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO decisions (
            id, session_id, workspace_id, repo_id, question, context_json, severity,
            tags_json, impact_repo_ids, assigned_to, agent_goal, created_at,
            resolved_at, resolution_json
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&decision.id)
    .bind(&decision.session_id)
    .bind(&decision.workspace_id)
    .bind(&decision.repo_id)
    .bind(&decision.question)
    .bind(serde_json::to_string(&decision.context)?)
    .bind(severity_to_str(decision.severity))
    .bind(serde_json::to_string(&decision.tags)?)
    .bind(serde_json::to_string(&decision.impact_repo_ids)?)
    .bind(&decision.assigned_to)
    .bind(&decision.agent_goal)
    .bind(decision.created_at.to_rfc3339())
    .bind(decision.resolved_at.map(|ts| ts.to_rfc3339()))
    .bind(match &decision.resolution {
        Some(resolution) => Some(serde_json::to_string(resolution)?),
        None => None,
    })
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn insert_replay_event(pool: &SqlitePool, event: &ReplayEvent) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO replay_events (
            session_id, repo_id, timestamp, elapsed, event_type, action, result, payload
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&event.session_id)
    .bind(&event.repo_id)
    .bind(event.timestamp.to_rfc3339())
    .bind(&event.elapsed)
    .bind(replay_type_to_str(&event.event_type))
    .bind(&event.action)
    .bind(&event.result)
    .bind(match &event.payload {
        Some(payload) => Some(serde_json::to_string(payload)?),
        None => None,
    })
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_replay_events(
    pool: &SqlitePool,
    session_id: &str,
    after: Option<u64>,
    limit: u32,
) -> anyhow::Result<Vec<ReplayEvent>> {
    let mut query = String::from(
        r#"
        SELECT id, session_id, repo_id, timestamp, elapsed, event_type, action, result, payload
        FROM replay_events
        WHERE session_id = ?
        "#,
    );
    if after.is_some() {
        query.push_str(" AND id > ?");
    }
    query.push_str(" ORDER BY id ASC LIMIT ?");

    let mut q = sqlx::query_as::<_, ReplayEventRow>(&query).bind(session_id);
    if let Some(after_id) = after {
        q = q.bind(after_id as i64);
    }
    let rows = q.bind(limit as i64).fetch_all(pool).await?;
    rows.into_iter().map(replay_event_from_row).collect()
}

pub async fn ensure_workspace(pool: &SqlitePool, workspace_id: &str) -> anyhow::Result<()> {
    let org_id = "org-default";
    sqlx::query("INSERT OR IGNORE INTO organizations (id, name) VALUES (?, ?)")
        .bind(org_id)
        .bind("Default Org")
        .execute(pool)
        .await?;
    sqlx::query(
        r#"
        INSERT OR IGNORE INTO workspaces
            (id, org_id, name, timezone, attention_budget_total, repos_json, members_json)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(workspace_id)
    .bind(org_id)
    .bind(workspace_id)
    .bind("UTC")
    .bind(12)
    .bind("[]")
    .bind("[]")
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn handoff_count(pool: &SqlitePool) -> anyhow::Result<u64> {
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM handoffs")
        .fetch_one(pool)
        .await?;
    Ok(count.0 as u64)
}

pub async fn insert_handoff(pool: &SqlitePool, handoff: &HandoffRecord) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO handoffs (
            id, role_from, role_to, status, session_id_from, artifact_type, summary,
            acceptance_json, github_owner, github_repo, github_issue_number, github_pr_number,
            created_at, updated_at, claimed_by, completed_by
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&handoff.id)
    .bind(role_to_str(handoff.role_from))
    .bind(role_to_str(handoff.role_to))
    .bind(handoff_status_to_str(handoff.status))
    .bind(&handoff.session_id_from)
    .bind(&handoff.artifact_type)
    .bind(&handoff.summary)
    .bind(serde_json::to_string(&handoff.acceptance_criteria)?)
    .bind(&handoff.github_owner)
    .bind(&handoff.github_repo)
    .bind(handoff.github_issue_number as i64)
    .bind(handoff.github_pr_number.map(|n| n as i64))
    .bind(handoff.created_at.to_rfc3339())
    .bind(handoff.updated_at.to_rfc3339())
    .bind(&handoff.claimed_by)
    .bind(&handoff.completed_by)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_handoff(pool: &SqlitePool, id: &str) -> anyhow::Result<Option<HandoffRecord>> {
    let row = sqlx::query_as::<_, HandoffRow>(
        r#"
        SELECT
            id, role_from, role_to, status, session_id_from, artifact_type, summary,
            acceptance_json, github_owner, github_repo, github_issue_number, github_pr_number,
            created_at, updated_at, claimed_by, completed_by
        FROM handoffs
        WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    row.map(handoff_from_row).transpose()
}

pub async fn update_handoff(pool: &SqlitePool, handoff: &HandoffRecord) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        UPDATE handoffs
        SET
            role_from = ?, role_to = ?, status = ?, session_id_from = ?, artifact_type = ?,
            summary = ?, acceptance_json = ?, github_owner = ?, github_repo = ?,
            github_issue_number = ?, github_pr_number = ?, created_at = ?, updated_at = ?,
            claimed_by = ?, completed_by = ?
        WHERE id = ?
        "#,
    )
    .bind(role_to_str(handoff.role_from))
    .bind(role_to_str(handoff.role_to))
    .bind(handoff_status_to_str(handoff.status))
    .bind(&handoff.session_id_from)
    .bind(&handoff.artifact_type)
    .bind(&handoff.summary)
    .bind(serde_json::to_string(&handoff.acceptance_criteria)?)
    .bind(&handoff.github_owner)
    .bind(&handoff.github_repo)
    .bind(handoff.github_issue_number as i64)
    .bind(handoff.github_pr_number.map(|n| n as i64))
    .bind(handoff.created_at.to_rfc3339())
    .bind(handoff.updated_at.to_rfc3339())
    .bind(&handoff.claimed_by)
    .bind(&handoff.completed_by)
    .bind(&handoff.id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_handoffs(
    pool: &SqlitePool,
    role: Option<Role>,
    status: Option<HandoffStatus>,
    github_owner: Option<&str>,
    github_repo: Option<&str>,
    github_issue_number: Option<u64>,
) -> anyhow::Result<Vec<HandoffRecord>> {
    let rows = sqlx::query_as::<_, HandoffRow>(
        r#"
        SELECT
            id, role_from, role_to, status, session_id_from, artifact_type, summary,
            acceptance_json, github_owner, github_repo, github_issue_number, github_pr_number,
            created_at, updated_at, claimed_by, completed_by
        FROM handoffs
        WHERE (?1 IS NULL OR role_to = ?1)
          AND (?2 IS NULL OR status = ?2)
          AND (?3 IS NULL OR github_owner = ?3)
          AND (?4 IS NULL OR github_repo = ?4)
          AND (?5 IS NULL OR github_issue_number = ?5)
        ORDER BY updated_at DESC
        "#,
    )
    .bind(role.map(role_to_str))
    .bind(status.map(handoff_status_to_str))
    .bind(github_owner)
    .bind(github_repo)
    .bind(github_issue_number.map(|n| n as i64))
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(handoff_from_row).collect()
}

pub async fn list_workspaces(pool: &SqlitePool) -> anyhow::Result<Vec<Workspace>> {
    let rows = sqlx::query_as::<_, WorkspaceRow>(
        r#"
        SELECT id, org_id, name, timezone, attention_budget_total, repos_json, members_json
        FROM workspaces
        ORDER BY id
        "#,
    )
    .fetch_all(pool)
    .await?;

    let mut out = Vec::new();
    for row in rows {
        out.push(workspace_from_row(pool, row).await?);
    }
    Ok(out)
}

pub async fn get_workspace(pool: &SqlitePool, id: &str) -> anyhow::Result<Option<Workspace>> {
    let row = sqlx::query_as::<_, WorkspaceRow>(
        r#"
        SELECT id, org_id, name, timezone, attention_budget_total, repos_json, members_json
        FROM workspaces
        WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    match row {
        Some(row) => Ok(Some(workspace_from_row(pool, row).await?)),
        None => Ok(None),
    }
}

pub async fn get_decision(pool: &SqlitePool, id: &str) -> anyhow::Result<Option<Decision>> {
    let row = sqlx::query_as::<_, DecisionRow>(
        r#"
        SELECT id, session_id, workspace_id, repo_id, question, context_json, severity,
               tags_json, impact_repo_ids, assigned_to, agent_goal, created_at,
               resolved_at, resolution_json
        FROM decisions
        WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    row.map(decision_from_row).transpose()
}

pub async fn list_decisions(
    pool: &SqlitePool,
    workspace_id: &str,
    repo_id: Option<&str>,
    status: Option<DecisionStatus>,
) -> anyhow::Result<Vec<Decision>> {
    let mut query = String::from(
        r#"
        SELECT id, session_id, workspace_id, repo_id, question, context_json, severity,
               tags_json, impact_repo_ids, assigned_to, agent_goal, created_at,
               resolved_at, resolution_json
        FROM decisions
        WHERE workspace_id = ?
        "#,
    );
    if repo_id.is_some() {
        query.push_str(" AND repo_id = ?");
    }
    if let Some(filter) = status {
        match filter {
            DecisionStatus::Pending => query.push_str(" AND resolved_at IS NULL"),
            DecisionStatus::Resolved => query.push_str(" AND resolved_at IS NOT NULL"),
        }
    }
    query.push_str(" ORDER BY created_at DESC");

    let mut q = sqlx::query_as::<_, DecisionRow>(&query).bind(workspace_id);
    if let Some(repo) = repo_id {
        q = q.bind(repo);
    }
    let rows = q.fetch_all(pool).await?;
    rows.into_iter().map(decision_from_row).collect()
}

pub async fn resolve_decision(
    pool: &SqlitePool,
    id: &str,
    resolution: &DecisionResolution,
) -> anyhow::Result<()> {
    let result = sqlx::query(
        r#"
        UPDATE decisions
        SET resolved_at = ?, resolution_json = ?
        WHERE id = ? AND resolved_at IS NULL
        "#,
    )
    .bind(resolution.resolved_at.to_rfc3339())
    .bind(serde_json::to_string(resolution)?)
    .bind(id)
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        anyhow::bail!("decision already resolved");
    }
    Ok(())
}

pub async fn log_budget_action(
    pool: &SqlitePool,
    workspace_id: &str,
    decision_id: &str,
    reviewer: &str,
    action: DecisionAction,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO attention_budget_log (workspace_id, decision_id, reviewer, action, logged_at)
        VALUES (?, ?, ?, ?, ?)
        "#,
    )
    .bind(workspace_id)
    .bind(decision_id)
    .bind(reviewer)
    .bind(decision_action_to_str(action))
    .bind(Utc::now().to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

#[allow(dead_code)]
pub async fn budget_used_today(
    pool: &SqlitePool,
    workspace_id: &str,
    timezone: &str,
) -> anyhow::Result<u32> {
    let tz: Tz = timezone
        .parse()
        .with_context(|| format!("invalid timezone: {timezone}"))?;
    let now = Utc::now().with_timezone(&tz);
    let today = NaiveDate::from_ymd_opt(now.year(), now.month(), now.day())
        .context("failed to compute today date")?;
    let start = tz
        .from_local_datetime(&today.and_hms_opt(0, 0, 0).unwrap())
        .single()
        .context("failed to compute start of day")?
        .with_timezone(&Utc)
        .to_rfc3339();

    let count: (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*) FROM attention_budget_log
        WHERE workspace_id = ? AND logged_at >= ?
        "#,
    )
    .bind(workspace_id)
    .bind(start)
    .fetch_one(pool)
    .await?;
    Ok(count.0 as u32)
}

pub async fn decision_count(pool: &SqlitePool) -> anyhow::Result<u64> {
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM decisions")
        .fetch_one(pool)
        .await?;
    Ok(count.0 as u64)
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct CachedSession {
    pub session_id: String,
    pub workspace_id: String,
    pub edge_id: String,
    pub hub_meta: SessionHubMeta,
    pub state: SessionState,
    pub updated_at: DateTime<Utc>,
}

#[allow(dead_code)]
pub async fn upsert_session_cache(
    pool: &SqlitePool,
    session_id: &str,
    workspace_id: &str,
    edge_id: &str,
    hub_meta: &SessionHubMeta,
    state: SessionState,
) -> anyhow::Result<()> {
    let meta_json = serde_json::to_string(hub_meta)?;
    let state_json = serde_json::to_string(&state)?;
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        INSERT INTO session_cache (session_id, workspace_id, edge_id, hub_meta_json, state, updated_at)
        VALUES (?, ?, ?, ?, ?, ?)
        ON CONFLICT(session_id) DO UPDATE SET
            workspace_id = excluded.workspace_id,
            edge_id = excluded.edge_id,
            hub_meta_json = excluded.hub_meta_json,
            state = excluded.state,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(session_id)
    .bind(workspace_id)
    .bind(edge_id)
    .bind(meta_json)
    .bind(state_json)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

#[allow(dead_code)]
pub async fn list_cached_sessions(
    pool: &SqlitePool,
    workspace_id: &str,
) -> anyhow::Result<Vec<CachedSession>> {
    let rows: Vec<(String, String, String, String, String, String)> = sqlx::query_as(
        r#"
        SELECT session_id, workspace_id, edge_id, hub_meta_json, state, updated_at
        FROM session_cache
        WHERE workspace_id = ?
        ORDER BY updated_at DESC
        "#,
    )
    .bind(workspace_id)
    .fetch_all(pool)
    .await?;

    let mut out = Vec::new();
    for (session_id, workspace_id, edge_id, meta_json, state_json, updated_at) in rows {
        let hub_meta: SessionHubMeta = serde_json::from_str(&meta_json)?;
        let state: SessionState = serde_json::from_str(&state_json)?;
        let updated_at = DateTime::parse_from_rfc3339(&updated_at)?.with_timezone(&Utc);
        out.push(CachedSession {
            session_id,
            workspace_id,
            edge_id,
            hub_meta,
            state,
            updated_at,
        });
    }
    Ok(out)
}

#[allow(dead_code)]
pub async fn mark_edge_sessions_unreachable(
    pool: &SqlitePool,
    edge_id: &str,
) -> anyhow::Result<u64> {
    let state_json = serde_json::to_string(&SessionState::Unreachable)?;
    let now = Utc::now().to_rfc3339();
    let result = sqlx::query(
        r#"
        UPDATE session_cache
        SET state = ?, updated_at = ?
        WHERE edge_id = ?
        "#,
    )
    .bind(state_json)
    .bind(now)
    .bind(edge_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

fn decision_from_row(row: DecisionRow) -> anyhow::Result<Decision> {
    let context: DecisionContext = serde_json::from_str(&row.context_json)?;
    let severity = severity_from_str(&row.severity)?;
    let tags: Vec<String> = serde_json::from_str(&row.tags_json)?;
    let impact_repo_ids: Vec<String> = serde_json::from_str(&row.impact_repo_ids)?;
    let created_at = DateTime::parse_from_rfc3339(&row.created_at)?.with_timezone(&Utc);
    let resolved_at = match row.resolved_at {
        Some(ts) => Some(DateTime::parse_from_rfc3339(&ts)?.with_timezone(&Utc)),
        None => None,
    };
    let resolution = match row.resolution_json {
        Some(json) => Some(serde_json::from_str(&json)?),
        None => None,
    };
    Ok(Decision {
        id: row.id,
        session_id: row.session_id,
        workspace_id: row.workspace_id,
        repo_id: row.repo_id,
        question: row.question,
        context,
        severity,
        tags,
        impact_repo_ids,
        assigned_to: row.assigned_to,
        agent_goal: row.agent_goal,
        created_at,
        resolved_at,
        resolution,
    })
}

async fn workspace_from_row(pool: &SqlitePool, row: WorkspaceRow) -> anyhow::Result<Workspace> {
    let repos: Vec<WorkspaceRepo> = serde_json::from_str(&row.repos_json)?;
    let members: Vec<String> = serde_json::from_str(&row.members_json)?;
    let used = budget_used_today(pool, &row.id, &row.timezone)
        .await
        .unwrap_or(0);
    Ok(Workspace {
        id: row.id,
        org_id: row.org_id,
        name: row.name,
        repos,
        members,
        attention_budget: AttentionBudget {
            used,
            total: row.attention_budget_total as u32,
            reset_tz: row.timezone,
        },
    })
}

fn replay_event_from_row(row: ReplayEventRow) -> anyhow::Result<ReplayEvent> {
    let timestamp = DateTime::parse_from_rfc3339(&row.timestamp)?.with_timezone(&Utc);
    Ok(ReplayEvent {
        id: row.id as u64,
        session_id: row.session_id,
        repo_id: row.repo_id,
        timestamp,
        elapsed: row.elapsed,
        event_type: replay_type_from_str(&row.event_type),
        action: row.action,
        result: row.result,
        payload: match row.payload {
            Some(payload) => Some(serde_json::from_str(&payload)?),
            None => None,
        },
    })
}

fn severity_to_str(severity: Severity) -> &'static str {
    match severity {
        Severity::Critical => "critical",
        Severity::High => "high",
        Severity::Medium => "medium",
        Severity::Low => "low",
    }
}

fn severity_from_str(value: &str) -> anyhow::Result<Severity> {
    match value {
        "critical" => Ok(Severity::Critical),
        "high" => Ok(Severity::High),
        "medium" => Ok(Severity::Medium),
        "low" => Ok(Severity::Low),
        other => anyhow::bail!("unknown severity: {other}"),
    }
}

fn replay_type_to_str(event_type: &ReplayEventType) -> &'static str {
    match event_type {
        ReplayEventType::System => "system",
        ReplayEventType::Read => "read",
        ReplayEventType::Edit => "edit",
        ReplayEventType::Tool => "tool",
        ReplayEventType::Switch => "switch",
        ReplayEventType::Test => "test",
        ReplayEventType::Decision => "decision",
    }
}

fn replay_type_from_str(raw: &str) -> ReplayEventType {
    match raw {
        "system" => ReplayEventType::System,
        "read" => ReplayEventType::Read,
        "edit" => ReplayEventType::Edit,
        "tool" => ReplayEventType::Tool,
        "switch" => ReplayEventType::Switch,
        "test" => ReplayEventType::Test,
        "decision" => ReplayEventType::Decision,
        _ => ReplayEventType::System,
    }
}

fn role_to_str(role: Role) -> &'static str {
    match role {
        Role::ProductManager => "product_manager",
        Role::Researcher => "researcher",
        Role::Designer => "designer",
        Role::Implementer => "implementer",
        Role::ReviewerTester => "reviewer_tester",
        Role::Sre => "sre",
    }
}

fn role_from_str(raw: &str) -> anyhow::Result<Role> {
    match raw {
        "product_manager" => Ok(Role::ProductManager),
        "researcher" => Ok(Role::Researcher),
        "designer" => Ok(Role::Designer),
        "implementer" => Ok(Role::Implementer),
        "reviewer_tester" => Ok(Role::ReviewerTester),
        "sre" => Ok(Role::Sre),
        _ => anyhow::bail!("unknown role: {raw}"),
    }
}

fn handoff_status_to_str(status: HandoffStatus) -> &'static str {
    match status {
        HandoffStatus::Queued => "queued",
        HandoffStatus::Claimed => "claimed",
        HandoffStatus::Completed => "completed",
        HandoffStatus::Rejected => "rejected",
        HandoffStatus::NeedsAttention => "needs_attention",
    }
}

fn handoff_status_from_str(raw: &str) -> anyhow::Result<HandoffStatus> {
    match raw {
        "queued" => Ok(HandoffStatus::Queued),
        "claimed" => Ok(HandoffStatus::Claimed),
        "completed" => Ok(HandoffStatus::Completed),
        "rejected" => Ok(HandoffStatus::Rejected),
        "needs_attention" => Ok(HandoffStatus::NeedsAttention),
        _ => anyhow::bail!("unknown handoff status: {raw}"),
    }
}

fn handoff_from_row(row: HandoffRow) -> anyhow::Result<HandoffRecord> {
    Ok(HandoffRecord {
        id: row.id,
        role_from: role_from_str(&row.role_from)?,
        role_to: role_from_str(&row.role_to)?,
        status: handoff_status_from_str(&row.status)?,
        session_id_from: row.session_id_from,
        artifact_type: row.artifact_type,
        summary: row.summary,
        acceptance_criteria: serde_json::from_str(&row.acceptance_json)?,
        github_owner: row.github_owner,
        github_repo: row.github_repo,
        github_issue_number: row.github_issue_number as u64,
        github_pr_number: row.github_pr_number.map(|n| n as u64),
        created_at: DateTime::parse_from_rfc3339(&row.created_at)?.with_timezone(&Utc),
        updated_at: DateTime::parse_from_rfc3339(&row.updated_at)?.with_timezone(&Utc),
        claimed_by: row.claimed_by,
        completed_by: row.completed_by,
    })
}

fn decision_action_to_str(action: DecisionAction) -> &'static str {
    match action {
        DecisionAction::Approve => "approve",
        DecisionAction::Deny => "deny",
        DecisionAction::Comment => "comment",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use forgemux_core::{
        DecisionContext, DecisionResolution, DiffLine, DiffLineType, ReplayEventType,
    };
    use tempfile::tempdir;

    fn sample_decision(id: &str) -> Decision {
        Decision {
            id: id.to_string(),
            session_id: "S-1234abcd".to_string(),
            workspace_id: "ws-1".to_string(),
            repo_id: "repo-1".to_string(),
            question: "Ship the change?".to_string(),
            context: DecisionContext::Diff {
                file: "src/lib.rs".to_string(),
                lines: vec![DiffLine {
                    line_type: DiffLineType::Add,
                    text: "println!(\"ok\");".to_string(),
                }],
            },
            severity: Severity::Medium,
            tags: vec!["deploy".to_string()],
            impact_repo_ids: vec!["repo-2".to_string()],
            assigned_to: None,
            agent_goal: "Ship release".to_string(),
            created_at: Utc::now(),
            resolved_at: None,
            resolution: None,
        }
    }

    fn sample_replay_event(session_id: &str, action: &str) -> ReplayEvent {
        ReplayEvent {
            id: 0,
            session_id: session_id.to_string(),
            repo_id: Some("repo-1".to_string()),
            timestamp: Utc::now(),
            elapsed: "1m".to_string(),
            event_type: ReplayEventType::Tool,
            action: action.to_string(),
            result: None,
            payload: None,
        }
    }

    async fn seed_workspace(pool: &SqlitePool) {
        sqlx::query("INSERT INTO organizations (id, name) VALUES (?, ?)")
            .bind("org-1")
            .bind("Forge")
            .execute(pool)
            .await
            .unwrap();
        sqlx::query(
            r#"
            INSERT INTO workspaces (id, org_id, name, timezone, attention_budget_total, repos_json, members_json)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind("ws-1")
        .bind("org-1")
        .bind("Core")
        .bind("UTC")
        .bind(12)
        .bind("[]")
        .bind("[]")
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn decision_crud_roundtrip() {
        let tmp = tempdir().unwrap();
        let pool = init_db(tmp.path()).await.unwrap();
        seed_workspace(&pool).await;

        let decision = sample_decision("D-0001");
        insert_decision(&pool, &decision).await.unwrap();

        let loaded = get_decision(&pool, "D-0001").await.unwrap().unwrap();
        assert_eq!(loaded.id, decision.id);
        assert_eq!(loaded.severity, decision.severity);

        let list = list_decisions(&pool, "ws-1", None, Some(DecisionStatus::Pending))
            .await
            .unwrap();
        assert_eq!(list.len(), 1);

        let resolution = DecisionResolution {
            action: DecisionAction::Approve,
            reviewer: "jono".to_string(),
            comment: Some("ship it".to_string()),
            resolved_at: Utc::now(),
        };
        resolve_decision(&pool, "D-0001", &resolution)
            .await
            .unwrap();

        let loaded = get_decision(&pool, "D-0001").await.unwrap().unwrap();
        assert!(loaded.resolved_at.is_some());
        assert_eq!(loaded.resolution.unwrap().action, DecisionAction::Approve);

        let pending = list_decisions(&pool, "ws-1", None, Some(DecisionStatus::Pending))
            .await
            .unwrap();
        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn resolve_decision_rejects_double_resolve() {
        let tmp = tempdir().unwrap();
        let pool = init_db(tmp.path()).await.unwrap();
        seed_workspace(&pool).await;

        let decision = sample_decision("D-0002");
        insert_decision(&pool, &decision).await.unwrap();

        let resolution = DecisionResolution {
            action: DecisionAction::Approve,
            reviewer: "jono".to_string(),
            comment: None,
            resolved_at: Utc::now(),
        };
        resolve_decision(&pool, "D-0002", &resolution)
            .await
            .unwrap();
        let err = resolve_decision(&pool, "D-0002", &resolution)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("already resolved"));
    }

    #[tokio::test]
    async fn budget_logging_counts_today() {
        let tmp = tempdir().unwrap();
        let pool = init_db(tmp.path()).await.unwrap();
        seed_workspace(&pool).await;

        log_budget_action(&pool, "ws-1", "D-0001", "jono", DecisionAction::Approve)
            .await
            .unwrap();

        let used = budget_used_today(&pool, "ws-1", "UTC").await.unwrap();
        assert_eq!(used, 1);
    }

    #[tokio::test]
    async fn list_decisions_filters_by_repo() {
        let tmp = tempdir().unwrap();
        let pool = init_db(tmp.path()).await.unwrap();
        seed_workspace(&pool).await;

        let mut d1 = sample_decision("D-0001");
        d1.repo_id = "repo-1".to_string();
        insert_decision(&pool, &d1).await.unwrap();

        let mut d2 = sample_decision("D-0002");
        d2.repo_id = "repo-2".to_string();
        insert_decision(&pool, &d2).await.unwrap();

        let list = list_decisions(&pool, "ws-1", Some("repo-1"), None)
            .await
            .unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "D-0001");
    }

    #[tokio::test]
    async fn session_cache_roundtrip_and_unreachable() {
        let tmp = tempdir().unwrap();
        let pool = init_db(tmp.path()).await.unwrap();
        seed_workspace(&pool).await;

        let meta = SessionHubMeta {
            workspace_id: "ws-1".to_string(),
            goal: "Ship".to_string(),
            risk: forgemux_core::RiskLevel::Green,
            context_pct: 10,
            touched_repos: vec![],
            pending_decisions: 0,
            tests_status: forgemux_core::TestsStatus::None,
            tokens_total: "0".to_string(),
            estimated_cost_usd: 0.0,
            lines_added: 0,
            lines_removed: 0,
            commits: 0,
        };

        upsert_session_cache(&pool, "S-1", "ws-1", "edge-1", &meta, SessionState::Running)
            .await
            .unwrap();

        let list = list_cached_sessions(&pool, "ws-1").await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].state, SessionState::Running);

        let updated = mark_edge_sessions_unreachable(&pool, "edge-1")
            .await
            .unwrap();
        assert_eq!(updated, 1);

        let list = list_cached_sessions(&pool, "ws-1").await.unwrap();
        assert_eq!(list[0].state, SessionState::Unreachable);
    }

    #[tokio::test]
    async fn replay_events_paginate() {
        let tmp = tempdir().unwrap();
        let pool = init_db(tmp.path()).await.unwrap();
        let event1 = sample_replay_event("S-1", "git status");
        let event2 = sample_replay_event("S-1", "cargo test");
        insert_replay_event(&pool, &event1).await.unwrap();
        insert_replay_event(&pool, &event2).await.unwrap();

        let first = list_replay_events(&pool, "S-1", None, 1).await.unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].action, "git status");

        let after = Some(first[0].id);
        let second = list_replay_events(&pool, "S-1", after, 10).await.unwrap();
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].action, "cargo test");
    }
}
