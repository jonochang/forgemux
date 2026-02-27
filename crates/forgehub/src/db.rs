use anyhow::Context;
use chrono::{DateTime, Datelike, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use forgemux_core::{Decision, DecisionAction, DecisionContext, DecisionResolution, Severity};
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
    sqlx::query(
        r#"
        UPDATE decisions
        SET resolved_at = ?, resolution_json = ?
        WHERE id = ?
        "#,
    )
    .bind(resolution.resolved_at.to_rfc3339())
    .bind(serde_json::to_string(resolution)?)
    .bind(id)
    .execute(pool)
    .await?;
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
    use forgemux_core::{DecisionContext, DecisionResolution, DiffLine, DiffLineType};
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
}
