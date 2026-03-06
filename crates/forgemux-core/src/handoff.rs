use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    ProductManager,
    Researcher,
    Designer,
    Implementer,
    ReviewerTester,
    Sre,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffStatus {
    Queued,
    Claimed,
    Completed,
    Rejected,
    NeedsAttention,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffOutcome {
    Approve,
    RequestChanges,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffRecord {
    pub id: String,
    pub role_from: Role,
    pub role_to: Role,
    pub status: HandoffStatus,
    pub session_id_from: Option<String>,
    pub artifact_type: String,
    pub summary: String,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
    pub github_owner: String,
    pub github_repo: String,
    pub github_issue_number: u64,
    pub github_pr_number: Option<u64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub claimed_by: Option<String>,
    pub completed_by: Option<String>,
}

pub fn is_transition_allowed(role_from: Role, role_to: Role) -> bool {
    matches!(
        (role_from, role_to),
        (Role::ProductManager, Role::Researcher)
            | (Role::Researcher, Role::Designer)
            | (Role::Designer, Role::Implementer)
            | (Role::Implementer, Role::ReviewerTester)
            | (Role::ReviewerTester, Role::Sre)
            | (Role::ReviewerTester, Role::Implementer)
    )
}

