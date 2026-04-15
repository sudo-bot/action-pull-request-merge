//! Trait abstraction over the GitHub REST API surface we need.
//!
//! Defining a trait keeps `octocrab` (the real HTTP client) at arm's length
//! so the action's decision logic can be unit-tested with a fake.

use anyhow::{anyhow, Context as _, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::inputs::MergeMethod;

/// Minimal projection of a pull request that the action actually inspects.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PullRequest {
    pub state: String,
    pub head: GitRef,
    pub base: GitRef,
    pub labels: Vec<Label>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GitRef {
    #[serde(rename = "ref")]
    pub ref_: String,
    pub sha: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Label {
    pub name: String,
}

/// Parameters accepted by the merge endpoint.
#[derive(Debug, Clone, Default, Serialize)]
pub struct MergeRequest {
    pub merge_method: &'static str,
    pub sha: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_message: Option<String>,
}

impl MergeRequest {
    pub fn from_inputs(method: MergeMethod, head_sha: &str, title: &str, message: &str) -> Self {
        Self {
            merge_method: match method {
                MergeMethod::Merge => "merge",
                MergeMethod::Squash => "squash",
                MergeMethod::Rebase => "rebase",
                // FastForward is handled out-of-band; defensively fall back to merge.
                MergeMethod::FastForward => "merge",
            },
            sha: head_sha.to_string(),
            commit_title: Some(title.to_string()).filter(|s| !s.trim().is_empty()),
            commit_message: Some(message.to_string()).filter(|s| !s.trim().is_empty()),
        }
    }
}

#[async_trait]
pub trait GithubClient: Send + Sync {
    async fn get_pull(&self, owner: &str, repo: &str, number: u64) -> Result<PullRequest>;

    /// Equivalent to `PATCH /repos/{owner}/{repo}/git/refs/{ref}` with
    /// `{ "sha": <sha>, "force": false }`. Used for fast-forward merges.
    async fn update_ref(
        &self,
        owner: &str,
        repo: &str,
        ref_: &str,
        sha: &str,
        force: bool,
    ) -> Result<()>;

    async fn merge_pull(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
        request: &MergeRequest,
    ) -> Result<()>;

    async fn remove_label(
        &self,
        owner: &str,
        repo: &str,
        issue_number: u64,
        label: &str,
    ) -> Result<()>;
}

/// Real implementation backed by `octocrab`.
pub struct OctocrabClient {
    inner: octocrab::Octocrab,
}

impl OctocrabClient {
    pub fn new(token: String, base_url: &str) -> Result<Self> {
        let mut builder = octocrab::Octocrab::builder().personal_token(token);
        // Allow GitHub Enterprise Server by honouring GITHUB_API_URL.
        if base_url != "https://api.github.com" {
            builder = builder
                .base_uri(base_url)
                .context("invalid GITHUB_API_URL")?;
        }
        let inner = builder.build().context("failed to build octocrab client")?;
        Ok(Self { inner })
    }
}

#[async_trait]
impl GithubClient for OctocrabClient {
    async fn get_pull(&self, owner: &str, repo: &str, number: u64) -> Result<PullRequest> {
        // Use the typed pulls API and re-serialise into our minimal shape.
        let pr = self
            .inner
            .pulls(owner, repo)
            .get(number)
            .await
            .with_context(|| format!("failed to fetch pull request #{}", number))?;
        let value = serde_json::to_value(&pr)?;
        Ok(
            serde_json::from_value(value)
                .context("pull request payload missing expected fields")?,
        )
    }

    async fn update_ref(
        &self,
        owner: &str,
        repo: &str,
        ref_: &str,
        sha: &str,
        force: bool,
    ) -> Result<()> {
        // octocrab does not expose a dedicated `update_ref`, so we use the
        // generic `_patch` helper to call the REST endpoint directly.
        let url = format!("/repos/{}/{}/git/refs/{}", owner, repo, ref_);
        let body = serde_json::json!({ "sha": sha, "force": force });
        let _resp: serde_json::Value = self
            .inner
            .patch(url, Some(&body))
            .await
            .with_context(|| format!("failed to update ref {}", ref_))?;
        Ok(())
    }

    async fn merge_pull(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
        request: &MergeRequest,
    ) -> Result<()> {
        let url = format!("/repos/{}/{}/pulls/{}/merge", owner, repo, number);
        let _resp: serde_json::Value = self
            .inner
            .put(url, Some(request))
            .await
            .with_context(|| format!("failed to merge pull request #{}", number))?;
        Ok(())
    }

    async fn remove_label(
        &self,
        owner: &str,
        repo: &str,
        issue_number: u64,
        label: &str,
    ) -> Result<()> {
        // The label name must be percent-encoded; do a minimal pass for the
        // characters likely to show up in label names.
        let encoded = label
            .replace('%', "%25")
            .replace(' ', "%20")
            .replace('/', "%2F");
        let url = format!(
            "/repos/{}/{}/issues/{}/labels/{}",
            owner, repo, issue_number, encoded
        );
        let _resp: serde_json::Value = self
            .inner
            .delete(url, None::<&()>)
            .await
            .map_err(|e| anyhow!("failed to remove label '{}': {}", label, e))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_request_serialises_required_fields_only() {
        let req = MergeRequest::from_inputs(MergeMethod::Squash, "deadbeef", "", "");
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["merge_method"], "squash");
        assert_eq!(json["sha"], "deadbeef");
        assert!(json.get("commit_title").is_none());
        assert!(json.get("commit_message").is_none());
    }

    #[test]
    fn merge_request_includes_title_and_message_when_provided() {
        let req = MergeRequest::from_inputs(MergeMethod::Merge, "abc", "T", "M");
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["commit_title"], "T");
        assert_eq!(json["commit_message"], "M");
    }

    #[test]
    fn merge_request_treats_whitespace_only_as_empty() {
        // Mirrors the JS check `.trim().length > 0`.
        let req = MergeRequest::from_inputs(MergeMethod::Merge, "abc", "   ", "\n\t");
        let json = serde_json::to_value(&req).unwrap();
        assert!(json.get("commit_title").is_none());
        assert!(json.get("commit_message").is_none());
    }

    #[test]
    fn merge_request_rebase_serialises_correctly() {
        let req = MergeRequest::from_inputs(MergeMethod::Rebase, "sha", "", "");
        assert_eq!(req.merge_method, "rebase");
    }
}
