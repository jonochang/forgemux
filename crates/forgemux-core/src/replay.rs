use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayEvent {
    pub id: u64,
    pub session_id: String,
    pub repo_id: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub elapsed: String,
    pub event_type: ReplayEventType,
    pub action: String,
    pub result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReplayEventType {
    System,
    Read,
    Edit,
    Tool,
    Switch,
    Test,
    Decision,
}
