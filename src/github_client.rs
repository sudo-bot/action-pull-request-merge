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
                // FastForward / FastForwardOrMerge are handled out-of-band; defensively fall back to merge.
                MergeMethod::FastForward | MergeMethod::FastForwardOrMerge => "merge",
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

    /// Advance `base_ref` to `head_sha` without creating a merge commit.
    ///
    /// The two backends do this on different endpoints:
    ///
    /// - **GitHub:** `PATCH /repos/{o}/{r}/git/refs/heads/{base_ref}` with
    ///   `{ "sha": <head_sha>, "force": false }`. Fails if the base is not
    ///   an ancestor of the head.
    /// - **Gitea:** `POST /repos/{o}/{r}/pulls/{n}/merge` with
    ///   `{ "Do": "fast-forward-only", "head_commit_id": <head_sha> }`.
    ///   Gitea has no `PATCH` on `/git/refs` (read-only API), so the
    ///   fast-forward goes through the merge endpoint with the dedicated
    ///   `Do` value (Gitea ≥ 1.22).
    async fn fast_forward(
        &self,
        owner: &str,
        repo: &str,
        pr_number: u64,
        base_ref: &str,
        head_sha: &str,
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
        // Deserialise directly into our minimal projection. Going through
        // octocrab's typed `pulls().get()` API would force a full GitHub
        // PullRequest shape (`node_id`, `html_url`, ...) that we don't need
        // and that some upstreams omit.
        let url = format!("/repos/{}/{}/pulls/{}", owner, repo, number);
        let pr: PullRequest = self
            .inner
            .get(url, None::<&()>)
            .await
            .with_context(|| format!("failed to fetch pull request #{}", number))?;
        Ok(pr)
    }

    async fn fast_forward(
        &self,
        owner: &str,
        repo: &str,
        _pr_number: u64,
        base_ref: &str,
        head_sha: &str,
    ) -> Result<()> {
        // GitHub's git-refs API supports moving a ref to a new commit via
        // PATCH. `force: false` ensures the move is rejected if it would
        // not be a true fast-forward (i.e. the base is not an ancestor of
        // the head), which is exactly the semantics we want.
        let url = format!("/repos/{}/{}/git/refs/heads/{}", owner, repo, base_ref);
        let body = serde_json::json!({ "sha": head_sha, "force": false });
        let _resp: serde_json::Value = self
            .inner
            .patch(url, Some(&body))
            .await
            .with_context(|| format!("failed to fast-forward heads/{}", base_ref))?;
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
        let url = format!(
            "/repos/{}/{}/issues/{}/labels/{}",
            owner,
            repo,
            issue_number,
            encode_path_segment(label),
        );
        let _resp: serde_json::Value = self
            .inner
            .delete(url, None::<&()>)
            .await
            .map_err(|e| anyhow!("failed to remove label '{}': {}", label, e))?;
        Ok(())
    }
}

/// Percent-encode a string for use as a single URL path segment.
///
/// Keeps the RFC 3986 *unreserved* set (`A–Z a–z 0–9 - _ . ~`) untouched and
/// percent-encodes every other byte. This is stricter than what's strictly
/// required by `pchar`, but it's the safe default — characters like `?`,
/// `#`, `&`, `+` and `/` would otherwise be interpreted by URL parsers as
/// query separators, fragment markers, or extra path segments and break the
/// request.
fn encode_path_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push_str(&format!("{:02X}", byte));
        }
    }
    out
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

    #[test]
    fn path_segment_keeps_unreserved_characters() {
        assert_eq!(encode_path_segment("merge-it"), "merge-it");
        assert_eq!(encode_path_segment("release_v1.2.3"), "release_v1.2.3");
        assert_eq!(encode_path_segment("a~b"), "a~b");
        assert_eq!(encode_path_segment(""), "");
    }

    #[test]
    fn path_segment_encodes_url_meta_characters() {
        // The previous implementation only handled `%`, space, and `/`,
        // which let labels containing `?`, `#`, `&` or `+` smuggle URL
        // syntax into the path and produce malformed requests.
        assert_eq!(encode_path_segment("a b"), "a%20b");
        assert_eq!(encode_path_segment("a/b"), "a%2Fb");
        assert_eq!(encode_path_segment("a?b"), "a%3Fb");
        assert_eq!(encode_path_segment("a#b"), "a%23b");
        assert_eq!(encode_path_segment("a&b"), "a%26b");
        assert_eq!(encode_path_segment("a+b"), "a%2Bb");
        assert_eq!(encode_path_segment("50%"), "50%25");
        assert_eq!(encode_path_segment("a=b"), "a%3Db");
        assert_eq!(encode_path_segment("a:b"), "a%3Ab");
    }

    #[test]
    fn path_segment_encodes_non_ascii_byte_by_byte() {
        // A label like "café" must encode the multi-byte UTF-8 sequence
        // for `é` (0xC3 0xA9) as %C3%A9 — anything else produces a request
        // that GitHub will reject.
        assert_eq!(encode_path_segment("café"), "caf%C3%A9");
        // Emoji: 🚀 is 0xF0 0x9F 0x9A 0x80.
        assert_eq!(encode_path_segment("🚀"), "%F0%9F%9A%80");
    }
}
