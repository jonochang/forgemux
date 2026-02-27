use forgemux_core::AgentType;
use regex::Regex;
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentLogSignal {
    WaitingInput,
}

pub trait AgentAdapter: Send + Sync {
    fn prompt_patterns(&self) -> &[Regex];
    fn log_paths(&self, cwd: &Path) -> Vec<PathBuf>;
    fn parse_log_line(&self, line: &str) -> Option<AgentLogSignal>;
}

#[derive(Debug, Clone)]
pub struct ConfigAdapter {
    agent: AgentType,
    prompt_patterns: Vec<Regex>,
    usage_paths: Vec<PathBuf>,
}

impl ConfigAdapter {
    pub fn new(agent: AgentType, prompt_patterns: Vec<Regex>, usage_paths: Vec<PathBuf>) -> Self {
        Self {
            agent,
            prompt_patterns,
            usage_paths,
        }
    }
}

impl AgentAdapter for ConfigAdapter {
    fn prompt_patterns(&self) -> &[Regex] {
        &self.prompt_patterns
    }

    fn log_paths(&self, cwd: &Path) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        let project_name = cwd
            .file_name()
            .map(|name| name.to_string_lossy().to_string());
        for base in &self.usage_paths {
            match self.agent {
                AgentType::Claude => {
                    if let Some(name) = project_name.as_ref() {
                        paths.push(base.join(name));
                    }
                    paths.push(base.clone());
                }
                AgentType::Codex => {
                    paths.push(base.clone());
                }
            }
        }
        paths
    }

    fn parse_log_line(&self, line: &str) -> Option<AgentLogSignal> {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return None;
        };
        let mut markers = Vec::new();
        collect_markers(&value, &mut markers);
        let matches = markers.iter().any(|marker| {
            matches!(
                marker.as_str(),
                "permission_request"
                    | "waiting_input"
                    | "input_required"
                    | "awaiting_input"
                    | "awaiting_user_input"
            )
        });
        if matches {
            Some(AgentLogSignal::WaitingInput)
        } else {
            None
        }
    }
}

fn collect_markers(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                if matches!(key.as_str(), "type" | "event" | "state")
                    && let Value::String(val) = value
                {
                    out.push(val.to_lowercase());
                }
                collect_markers(value, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_markers(item, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_log_line_detects_waiting_input() {
        let adapter =
            ConfigAdapter::new(AgentType::Claude, Vec::new(), vec![PathBuf::from("/tmp")]);
        let line = r#"{"type":"permission_request","detail":"approve?"}"#;
        assert_eq!(
            adapter.parse_log_line(line),
            Some(AgentLogSignal::WaitingInput)
        );
    }
}
