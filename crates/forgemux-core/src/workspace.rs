use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Organization {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceRepo {
    pub id: String,
    pub label: String,
    pub icon: String,
    pub color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AttentionBudget {
    pub used: u32,
    pub total: u32,
    pub reset_tz: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Workspace {
    pub id: String,
    pub org_id: String,
    pub name: String,
    pub repos: Vec<WorkspaceRepo>,
    pub members: Vec<String>,
    pub attention_budget: AttentionBudget,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_types_roundtrip() {
        let workspace = Workspace {
            id: "ws-1".to_string(),
            org_id: "org-1".to_string(),
            name: "Core Platform".to_string(),
            repos: vec![WorkspaceRepo {
                id: "repo-1".to_string(),
                label: "forgemux".to_string(),
                icon: "hammer".to_string(),
                color: "#111111".to_string(),
            }],
            members: vec!["jono".to_string()],
            attention_budget: AttentionBudget {
                used: 4,
                total: 10,
                reset_tz: "Australia/Melbourne".to_string(),
            },
        };

        let json = serde_json::to_string(&workspace).unwrap();
        let decoded: Workspace = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, workspace);
    }

    #[test]
    fn organization_roundtrip() {
        let org = Organization {
            id: "org-1".to_string(),
            name: "Forge".to_string(),
        };
        let json = serde_json::to_string(&org).unwrap();
        let decoded: Organization = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, org);
    }
}
