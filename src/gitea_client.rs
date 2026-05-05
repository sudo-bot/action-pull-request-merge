//! Gitea-specific implementation of [`GithubClient`].
//!
//! Gitea's REST API is GitHub-shaped but differs in two places that matter
//! to this action:
//!
//! - Pull-request merge uses `POST` (not `PUT`) and a different body schema:
//!   `Do` instead of `merge_method`, `MergeTitleField`/`MergeMessageField`
//!   instead of `commit_title`/`commit_message`, and `head_commit_id`
//!   instead of `sha`.
//! - Issue label removal requires the numeric label *id*, not its name, so
//!   the issue's labels must be fetched first to map name → id.
//!
//! The remaining endpoints (`GET /repos/.../pulls/{n}` and
//! `PATCH /repos/.../git/refs/{ref}`) are wire-compatible with GitHub and
//! are reused as-is.
//!
//! We piggyback on `octocrab` purely as an authenticated HTTP client. The
//! `Authorization: token <pat>` header it sets is accepted by Gitea, and we
//! never call any of octocrab's typed GitHub helpers.

use anyhow::{anyhow, Context as _, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::github_client::{GithubClient, MergeRequest, PullRequest};

pub struct GiteaClient {
    inner: octocrab::Octocrab,
}

impl GiteaClient {
    pub fn new(token: String, base_url: &str) -> Result<Self> {
        let inner = octocrab::Octocrab::builder()
            .personal_token(token)
            .base_uri(base_url)
            .context("invalid GITHUB_API_URL for Gitea")?
            .build()
            .context("failed to build octocrab client for Gitea")?;
        Ok(Self { inner })
    }
}

/// Body of `POST /repos/{owner}/{repo}/pulls/{index}/merge`.
///
/// Gitea uses CamelCase for the title/message fields and `Do` for the merge
/// strategy. `head_commit_id` is the Gitea equivalent of GitHub's `sha`.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct GiteaMergeBody {
    #[serde(rename = "Do")]
    pub do_: &'static str,
    #[serde(rename = "MergeTitleField", skip_serializing_if = "Option::is_none")]
    pub merge_title_field: Option<String>,
    #[serde(rename = "MergeMessageField", skip_serializing_if = "Option::is_none")]
    pub merge_message_field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head_commit_id: Option<String>,
}

impl GiteaMergeBody {
    pub(crate) fn from_github(req: &MergeRequest) -> Self {
        Self {
            // GitHub's `merge`/`squash`/`rebase` map 1:1 onto Gitea's `Do`.
            do_: req.merge_method,
            merge_title_field: req.commit_title.clone(),
            merge_message_field: req.commit_message.clone(),
            head_commit_id: Some(req.sha.clone()),
        }
    }
}

#[derive(Debug, Deserialize)]
struct GiteaLabel {
    id: i64,
    name: String,
}

#[async_trait]
impl GithubClient for GiteaClient {
    async fn get_pull(&self, owner: &str, repo: &str, number: u64) -> Result<PullRequest> {
        // Wire-compatible with GitHub's GET pull request endpoint.
        let url = format!("/repos/{}/{}/pulls/{}", owner, repo, number);
        let pr: PullRequest = self
            .inner
            .get(url, None::<&()>)
            .await
            .with_context(|| format!("failed to fetch pull request #{}", number))?;
        Ok(pr)
    }

    async fn update_ref(
        &self,
        owner: &str,
        repo: &str,
        ref_: &str,
        sha: &str,
        force: bool,
    ) -> Result<()> {
        // PATCH /repos/{o}/{r}/git/refs/{ref} — same shape on both servers.
        let url = format!("/repos/{}/{}/git/refs/{}", owner, repo, ref_);
        let body = serde_json::json!({ "sha": sha, "force": force });
        let resp = self
            .inner
            ._patch(url, Some(&body))
            .await
            .with_context(|| format!("failed to update ref {}", ref_))?;
        ensure_success(resp.status().as_u16(), "update ref")
    }

    async fn merge_pull(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
        request: &MergeRequest,
    ) -> Result<()> {
        let url = format!("/repos/{}/{}/pulls/{}/merge", owner, repo, number);
        let body = GiteaMergeBody::from_github(request);
        // Use the low-level helper because Gitea's merge endpoint returns
        // 200 with an empty body, which the typed helper would treat as a
        // deserialisation error.
        let resp = self
            .inner
            ._post(url, Some(&body))
            .await
            .with_context(|| format!("failed to merge pull request #{}", number))?;
        ensure_success(resp.status().as_u16(), "merge pull request")
    }

    async fn remove_label(
        &self,
        owner: &str,
        repo: &str,
        issue_number: u64,
        label: &str,
    ) -> Result<()> {
        // Gitea's DELETE wants the numeric label *id*, not the name. List
        // the issue's labels first and resolve.
        let labels_url = format!("/repos/{}/{}/issues/{}/labels", owner, repo, issue_number);
        let labels: Vec<GiteaLabel> = self
            .inner
            .get(labels_url, None::<&()>)
            .await
            .with_context(|| format!("failed to list labels on issue #{}", issue_number))?;
        let id = labels
            .iter()
            .find(|l| l.name == label)
            .map(|l| l.id)
            .ok_or_else(|| anyhow!("label '{}' not found on issue #{}", label, issue_number))?;
        let url = format!(
            "/repos/{}/{}/issues/{}/labels/{}",
            owner, repo, issue_number, id
        );
        let resp = self
            .inner
            ._delete(url, None::<&()>)
            .await
            .map_err(|e| anyhow!("failed to remove label '{}': {}", label, e))?;
        ensure_success(resp.status().as_u16(), "remove label")
    }
}

fn ensure_success(status: u16, what: &str) -> Result<()> {
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(anyhow!("Gitea {} returned HTTP {}", what, status))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github_client::PullRequest;
    use crate::inputs::MergeMethod;

    #[test]
    fn merge_body_uses_camel_case_and_head_commit_id() {
        let req = MergeRequest::from_inputs(MergeMethod::Squash, "deadbeef", "T", "M");
        let body = GiteaMergeBody::from_github(&req);
        let json = serde_json::to_value(&body).unwrap();
        assert_eq!(json["Do"], "squash");
        assert_eq!(json["MergeTitleField"], "T");
        assert_eq!(json["MergeMessageField"], "M");
        assert_eq!(json["head_commit_id"], "deadbeef");
        // No GitHub-style fields should leak.
        assert!(json.get("merge_method").is_none());
        assert!(json.get("commit_title").is_none());
        assert!(json.get("sha").is_none());
    }

    #[test]
    fn merge_body_omits_empty_title_and_message() {
        // MergeRequest::from_inputs already drops whitespace-only values to
        // None; the Gitea body should follow suit.
        let req = MergeRequest::from_inputs(MergeMethod::Merge, "abc", "", "");
        let body = GiteaMergeBody::from_github(&req);
        let json = serde_json::to_value(&body).unwrap();
        assert_eq!(json["Do"], "merge");
        assert_eq!(json["head_commit_id"], "abc");
        assert!(json.get("MergeTitleField").is_none());
        assert!(json.get("MergeMessageField").is_none());
    }

    #[test]
    fn merge_body_passes_rebase_through_unchanged() {
        // Gitea has both `rebase` (no merge commit) and `rebase-merge` (with
        // a merge commit). GitHub's `rebase` is the no-merge-commit variant,
        // so passing the GitHub name through is correct.
        let req = MergeRequest::from_inputs(MergeMethod::Rebase, "abc", "", "");
        let body = GiteaMergeBody::from_github(&req);
        let json = serde_json::to_value(&body).unwrap();
        assert_eq!(json["Do"], "rebase");
    }

    #[test]
    fn merge_body_handles_asymmetric_title_and_message() {
        // Title only: message must stay absent, not become an empty string.
        let req = MergeRequest::from_inputs(MergeMethod::Merge, "sha", "Just a title", "");
        let json = serde_json::to_value(GiteaMergeBody::from_github(&req)).unwrap();
        assert_eq!(json["MergeTitleField"], "Just a title");
        assert!(json.get("MergeMessageField").is_none());

        // Message only: mirror situation.
        let req = MergeRequest::from_inputs(MergeMethod::Merge, "sha", "", "Body only");
        let json = serde_json::to_value(GiteaMergeBody::from_github(&req)).unwrap();
        assert!(json.get("MergeTitleField").is_none());
        assert_eq!(json["MergeMessageField"], "Body only");
    }

    #[test]
    fn merge_body_fast_forward_inputs_collapse_to_plain_merge() {
        // `MergeRequest::from_inputs` already collapses FastForward/
        // FastForwardOrMerge to the `merge` strategy on the assumption that
        // the action layer has handled the FF path out-of-band. Make sure
        // that fallback shows up correctly in Gitea's `Do` field too —
        // otherwise we'd silently send `Do: "fast-forward"` (not a Gitea
        // value) and the merge would 422.
        for m in [MergeMethod::FastForward, MergeMethod::FastForwardOrMerge] {
            let req = MergeRequest::from_inputs(m, "sha", "", "");
            let json = serde_json::to_value(GiteaMergeBody::from_github(&req)).unwrap();
            assert_eq!(json["Do"], "merge", "method {:?} should fall back", m);
        }
    }

    #[test]
    fn merge_body_passes_plain_merge_through() {
        let req = MergeRequest::from_inputs(MergeMethod::Merge, "sha", "", "");
        let json = serde_json::to_value(GiteaMergeBody::from_github(&req)).unwrap();
        assert_eq!(json["Do"], "merge");
    }

    #[test]
    fn merge_body_serializes_only_known_keys() {
        // Lock down the wire shape so a future field addition is a deliberate
        // change — Gitea is strict about unknown body keys on some endpoints.
        let req = MergeRequest::from_inputs(MergeMethod::Squash, "sha", "T", "M");
        let json = serde_json::to_value(GiteaMergeBody::from_github(&req)).unwrap();
        let mut keys: Vec<&str> = json
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort();
        assert_eq!(
            keys,
            vec![
                "Do",
                "MergeMessageField",
                "MergeTitleField",
                "head_commit_id"
            ],
        );
    }

    #[test]
    fn ensure_success_accepts_2xx_and_rejects_others() {
        for ok in [200, 201, 202, 204, 299] {
            assert!(
                ensure_success(ok, "x").is_ok(),
                "{} should be treated as success",
                ok
            );
        }
        for fail in [199, 300, 301, 400, 401, 404, 422, 500, 502] {
            let err = ensure_success(fail, "merge").unwrap_err().to_string();
            assert!(err.contains(&fail.to_string()));
            assert!(err.contains("merge"));
        }
    }

    #[tokio::test]
    async fn gitea_client_new_rejects_invalid_base_url() {
        // Garbage that octocrab can't parse as a URI must produce an error
        // instead of panicking or building a half-broken client.
        let result = GiteaClient::new("token".into(), "::not a uri::");
        let err = result.err().expect("garbage URL must not build a client");
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("invalid GITHUB_API_URL")
                || msg.to_lowercase().contains("uri")
                || msg.to_lowercase().contains("url"),
            "unexpected error message: {}",
            msg
        );
    }

    #[tokio::test]
    async fn gitea_client_new_accepts_typical_gitea_base_url() {
        // Smoke test: building succeeds for the URL Gitea Actions hands out.
        // Needs a tokio runtime because octocrab's build() instantiates a
        // tower service that registers with the reactor.
        let result = GiteaClient::new("token".into(), "https://gitea.example.com/api/v1");
        assert!(result.is_ok(), "expected build to succeed for valid URL");
    }

    #[test]
    fn gitea_label_ignores_extra_fields() {
        // Gitea's labels carry color, description, exclusive, etc. Our
        // GiteaLabel projection must keep accepting payloads as those grow.
        let json = serde_json::json!({
            "id": 42,
            "name": "merge-it",
            "color": "00ff00",
            "description": "approved",
            "exclusive": false,
            "url": "https://gitea.example.com/api/v1/repos/o/r/labels/42",
            "is_archived": false,
        });
        let label: GiteaLabel = serde_json::from_value(json).unwrap();
        assert_eq!(label.id, 42);
        assert_eq!(label.name, "merge-it");
    }

    #[test]
    fn pull_request_deserialises_from_gitea_shaped_payload() {
        // Gitea returns a much richer PR object than our minimal projection;
        // make sure the four fields we care about (state, head{ref,sha},
        // base{ref,sha}, labels[].name) deserialise from a realistic Gitea
        // payload without choking on the surrounding noise.
        let payload = serde_json::json!({
            "id": 5,
            "number": 7,
            "state": "open",
            "title": "Add Gitea support",
            "user": { "id": 1, "login": "alice" },
            "head": {
                "label": "alice:topic",
                "ref": "topic",
                "sha": "deadbeefcafebabe",
                "repo_id": 100,
                "repo": null,
            },
            "base": {
                "label": "octo:main",
                "ref": "main",
                "sha": "0000000000000000",
                "repo_id": 100,
                "repo": null,
            },
            "labels": [
                {
                    "id": 1,
                    "name": "merge-it",
                    "color": "00ff00",
                    "description": "",
                },
                {
                    "id": 2,
                    "name": "release",
                    "color": "0000ff",
                    "description": "",
                },
            ],
            "milestone": null,
            "assignee": null,
            "mergeable": true,
            "merged": false,
        });
        let pr: PullRequest = serde_json::from_value(payload).unwrap();
        assert_eq!(pr.state, "open");
        assert_eq!(pr.head.ref_, "topic");
        assert_eq!(pr.head.sha, "deadbeefcafebabe");
        assert_eq!(pr.base.ref_, "main");
        assert_eq!(pr.labels.len(), 2);
        assert_eq!(pr.labels[0].name, "merge-it");
        assert_eq!(pr.labels[1].name, "release");
    }
}
