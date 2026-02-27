use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionHubMeta {
    pub workspace_id: String,
    pub goal: String,
    pub risk: RiskLevel,
    pub context_pct: u8,
    pub touched_repos: Vec<String>,
    pub pending_decisions: u32,
    pub tests_status: TestsStatus,
    pub tokens_total: String,
    pub estimated_cost_usd: f64,
    pub lines_added: u32,
    pub lines_removed: u32,
    pub commits: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Green,
    Yellow,
    Red,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TestsStatus {
    Passing,
    Failing,
    Pending,
    None,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_roundtrip() {
        let meta = SessionHubMeta {
            workspace_id: "ws-1".to_string(),
            goal: "Ship release".to_string(),
            risk: RiskLevel::Yellow,
            context_pct: 70,
            touched_repos: vec!["repo-1".to_string()],
            pending_decisions: 2,
            tests_status: TestsStatus::Pending,
            tokens_total: "12k".to_string(),
            estimated_cost_usd: 1.23,
            lines_added: 120,
            lines_removed: 20,
            commits: 2,
        };
        let json = serde_json::to_string(&meta).unwrap();
        let decoded: SessionHubMeta = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, meta);
    }
}
