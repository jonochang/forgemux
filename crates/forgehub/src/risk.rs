use chrono::{DateTime, Duration, Utc};
use forgemux_core::{Decision, RiskLevel, SessionHubMeta, SessionState, TestsStatus};

#[allow(dead_code)]
pub fn compute_risk(
    meta: &SessionHubMeta,
    pending: &[Decision],
    session_state: SessionState,
    now: DateTime<Utc>,
) -> RiskLevel {
    if session_state == SessionState::Errored {
        return RiskLevel::Red;
    }

    let oldest_pending = pending.iter().map(|d| d.created_at).min();
    if meta.context_pct > 85
        || oldest_pending
            .map(|ts| now - ts > Duration::minutes(15))
            .unwrap_or(false)
    {
        return RiskLevel::Red;
    }

    if meta.context_pct >= 70
        || meta.tests_status == TestsStatus::Failing
        || oldest_pending
            .map(|ts| now - ts > Duration::minutes(5))
            .unwrap_or(false)
    {
        return RiskLevel::Yellow;
    }

    RiskLevel::Green
}

#[cfg(test)]
mod tests {
    use super::*;
    use forgemux_core::{DecisionContext, Severity};

    fn meta_with(context_pct: u8, tests_status: TestsStatus) -> SessionHubMeta {
        SessionHubMeta {
            workspace_id: "ws-1".to_string(),
            goal: "Ship".to_string(),
            risk: RiskLevel::Green,
            context_pct,
            touched_repos: vec![],
            pending_decisions: 0,
            tests_status,
            tokens_total: "0".to_string(),
            estimated_cost_usd: 0.0,
            lines_added: 0,
            lines_removed: 0,
            commits: 0,
        }
    }

    fn decision_at(minutes_ago: i64) -> Decision {
        Decision {
            id: "D-1".to_string(),
            session_id: "S-1".to_string(),
            workspace_id: "ws-1".to_string(),
            repo_id: "repo-1".to_string(),
            question: "Ship?".to_string(),
            context: DecisionContext::Log {
                text: "hi".to_string(),
            },
            severity: Severity::Low,
            tags: vec![],
            impact_repo_ids: vec![],
            assigned_to: None,
            agent_goal: "Ship".to_string(),
            created_at: Utc::now() - Duration::minutes(minutes_ago),
            resolved_at: None,
            resolution: None,
        }
    }

    #[test]
    fn risk_red_when_errored() {
        let meta = meta_with(10, TestsStatus::Passing);
        let risk = compute_risk(&meta, &[], SessionState::Errored, Utc::now());
        assert_eq!(risk, RiskLevel::Red);
    }

    #[test]
    fn risk_red_on_high_context_or_old_decision() {
        let meta = meta_with(90, TestsStatus::Passing);
        let risk = compute_risk(&meta, &[], SessionState::Running, Utc::now());
        assert_eq!(risk, RiskLevel::Red);

        let meta = meta_with(10, TestsStatus::Passing);
        let risk = compute_risk(&meta, &[decision_at(20)], SessionState::Running, Utc::now());
        assert_eq!(risk, RiskLevel::Red);
    }

    #[test]
    fn risk_yellow_on_medium_signals() {
        let meta = meta_with(70, TestsStatus::Passing);
        let risk = compute_risk(&meta, &[], SessionState::Running, Utc::now());
        assert_eq!(risk, RiskLevel::Yellow);

        let meta = meta_with(10, TestsStatus::Failing);
        let risk = compute_risk(&meta, &[], SessionState::Running, Utc::now());
        assert_eq!(risk, RiskLevel::Yellow);

        let meta = meta_with(10, TestsStatus::Passing);
        let risk = compute_risk(&meta, &[decision_at(6)], SessionState::Running, Utc::now());
        assert_eq!(risk, RiskLevel::Yellow);
    }

    #[test]
    fn risk_green_when_healthy() {
        let meta = meta_with(20, TestsStatus::Passing);
        let risk = compute_risk(&meta, &[], SessionState::Running, Utc::now());
        assert_eq!(risk, RiskLevel::Green);
    }
}
