use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DecisionContext {
    Diff { file: String, lines: Vec<DiffLine> },
    Log { text: String },
    Screenshot { description: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiffLine {
    pub line_type: DiffLineType,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiffLineType {
    Ctx,
    Add,
    Del,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Critical = 0,
    High = 1,
    Medium = 2,
    Low = 3,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Decision {
    pub id: String,
    pub session_id: String,
    pub workspace_id: String,
    pub repo_id: String,
    pub question: String,
    pub context: DecisionContext,
    pub severity: Severity,
    pub tags: Vec<String>,
    pub impact_repo_ids: Vec<String>,
    pub assigned_to: Option<String>,
    pub agent_goal: String,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub resolution: Option<DecisionResolution>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DecisionResolution {
    pub action: DecisionAction,
    pub reviewer: String,
    pub comment: Option<String>,
    pub resolved_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DecisionAction {
    Approve,
    Deny,
    Comment,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_orders_by_priority() {
        assert!(Severity::Critical < Severity::High);
        assert!(Severity::High < Severity::Medium);
        assert!(Severity::Medium < Severity::Low);
    }

    #[test]
    fn decision_roundtrip() {
        let decision = Decision {
            id: "D-0001".to_string(),
            session_id: "S-1234abcd".to_string(),
            workspace_id: "ws-1".to_string(),
            repo_id: "repo-1".to_string(),
            question: "Ship the migration?".to_string(),
            context: DecisionContext::Diff {
                file: "src/main.rs".to_string(),
                lines: vec![
                    DiffLine {
                        line_type: DiffLineType::Ctx,
                        text: "fn main() {".to_string(),
                    },
                    DiffLine {
                        line_type: DiffLineType::Add,
                        text: "println!(\"hi\");".to_string(),
                    },
                ],
            },
            severity: Severity::High,
            tags: vec!["migration".to_string()],
            impact_repo_ids: vec!["repo-2".to_string()],
            assigned_to: Some("jono".to_string()),
            agent_goal: "Ship release".to_string(),
            created_at: Utc::now(),
            resolved_at: None,
            resolution: None,
        };

        let json = serde_json::to_string(&decision).unwrap();
        let decoded: Decision = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, decision);
    }
}
