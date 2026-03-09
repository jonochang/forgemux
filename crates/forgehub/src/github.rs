use anyhow::Context;
use reqwest::StatusCode;
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct GitHubClient {
    http: reqwest::Client,
    base_url: String,
    token: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitHubIssue {
    pub state: String,
}

impl GitHubClient {
    pub fn new(base_url: impl Into<String>, token: Option<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            token,
        }
    }

    pub async fn get_issue(
        &self,
        owner: &str,
        repo: &str,
        issue_number: u64,
    ) -> anyhow::Result<Option<GitHubIssue>> {
        let url = format!(
            "{}/repos/{owner}/{repo}/issues/{issue_number}",
            self.base_url
        );
        let req = self
            .http
            .get(url)
            .header("User-Agent", "forgemux-forgehub")
            .header("Accept", "application/vnd.github+json");
        let req = if let Some(token) = &self.token {
            req.bearer_auth(token)
        } else {
            req
        };
        let resp = req.send().await.context("github issue lookup failed")?;
        if resp.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !resp.status().is_success() {
            anyhow::bail!("github issue lookup failed: {}", resp.status());
        }
        Ok(Some(resp.json::<GitHubIssue>().await?))
    }

    pub async fn post_issue_comment(
        &self,
        owner: &str,
        repo: &str,
        issue_number: u64,
        comment: &str,
    ) -> anyhow::Result<()> {
        let url = format!(
            "{}/repos/{owner}/{repo}/issues/{issue_number}/comments",
            self.base_url
        );
        let req = self
            .http
            .post(url)
            .header("User-Agent", "forgemux-forgehub")
            .header("Accept", "application/vnd.github+json")
            .json(&serde_json::json!({ "body": comment }));
        let req = if let Some(token) = &self.token {
            req.bearer_auth(token)
        } else {
            req
        };
        let resp = req.send().await.context("github comment write failed")?;
        if !resp.status().is_success() {
            anyhow::bail!("github comment write failed: {}", resp.status());
        }
        Ok(())
    }
}
