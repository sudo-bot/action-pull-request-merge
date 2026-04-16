//! Decision logic for the action. Mirrors the original `index.js`.
//!
//! Kept entirely free of process-level concerns (env, stdout, exits) so it
//! can be exercised end-to-end with a fake [`GithubClient`] and a capturing
//! [`Logger`].

use anyhow::{Context as _, Result};
use regex::Regex;

use crate::context::GithubContext;
use crate::github_client::{GithubClient, MergeRequest, PullRequest};
use crate::inputs::{ActionInputs, MergeMethod};
use crate::logger::Logger;

/// Outcome of a single action run, useful for tests and observability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Merge skipped because the actor's username did not match the regex.
    SkippedActor,
    /// Merge skipped because the pull-request is closed.
    SkippedClosed,
    /// Merge skipped because the configured label is not on the PR.
    SkippedLabelMissing,
    /// Branch was fast-forwarded to the head SHA.
    FastForwarded,
    /// PR was merged via the merge/squash/rebase API.
    Merged,
}

pub async fn run<C: GithubClient + ?Sized, L: Logger + ?Sized>(
    client: &C,
    inputs: &ActionInputs,
    ctx: &GithubContext,
    log: &mut L,
) -> Result<Outcome> {
    // ---- 1. Username gate -------------------------------------------------
    let username_re = Regex::new(&inputs.allowed_usernames_regex).with_context(|| {
        format!(
            "invalid allowed-usernames-regex: {}",
            inputs.allowed_usernames_regex
        )
    })?;
    if !username_re.is_match(&ctx.actor) {
        log.warning("Ignored, the username does not match.");
        return Ok(Outcome::SkippedActor);
    }
    log.info("Username matched.");

    // ---- 2. Fetch the pull request ---------------------------------------
    let pr: PullRequest = client
        .get_pull(&ctx.owner, &ctx.repo, inputs.number)
        .await
        .context("failed to fetch pull request")?;

    if pr.state == "closed" {
        log.warning("Ignored, the pull-request is closed.");
        return Ok(Outcome::SkippedClosed);
    }
    log.info("The pull-request is open.");

    // ---- 3. Optional label gate ------------------------------------------
    if !inputs.filter_label.is_empty() {
        let label_re = Regex::new(&inputs.filter_label)
            .with_context(|| format!("invalid filter-label regex: {}", inputs.filter_label))?;
        let matched = pr.labels.iter().any(|l| label_re.is_match(&l.name));
        if !matched {
            log.warning("Ignored, the label does not exist on the pull-request.");
            return Ok(Outcome::SkippedLabelMissing);
        }
        log.info("Label matched.");
    } else {
        log.info("Label check is disabled.");
    }

    // ---- 4. Perform the merge / fast-forward -----------------------------
    let outcome = match inputs.merge_method {
        MergeMethod::FastForward => {
            log.info(&format!(
                "Updating to: heads/{}@{}",
                pr.base.ref_, pr.head.sha
            ));
            client
                .update_ref(
                    &ctx.owner,
                    &ctx.repo,
                    &format!("heads/{}", pr.base.ref_),
                    &pr.head.sha,
                    false,
                )
                .await
                .context("fast-forward update_ref failed")?;
            Outcome::FastForwarded
        }
        MergeMethod::FastForwardOrMerge => {
            log.info(&format!(
                "Attempting fast-forward: heads/{}@{}",
                pr.base.ref_, pr.head.sha
            ));
            match client
                .update_ref(
                    &ctx.owner,
                    &ctx.repo,
                    &format!("heads/{}", pr.base.ref_),
                    &pr.head.sha,
                    false,
                )
                .await
            {
                Ok(()) => {
                    log.info("Fast-forward succeeded.");
                    Outcome::FastForwarded
                }
                Err(e) => {
                    log.warning(&format!(
                        "Fast-forward failed ({}), falling back to merge.",
                        e
                    ));
                    let req = MergeRequest::from_inputs(
                        MergeMethod::Merge,
                        &pr.head.sha,
                        &inputs.merge_title,
                        &inputs.merge_message,
                    );
                    client
                        .merge_pull(&ctx.owner, &ctx.repo, inputs.number, &req)
                        .await
                        .context("merge request failed (after fast-forward fallback)")?;
                    Outcome::Merged
                }
            }
        }
        method => {
            let req = MergeRequest::from_inputs(
                method,
                &pr.head.sha,
                &inputs.merge_title,
                &inputs.merge_message,
            );
            client
                .merge_pull(&ctx.owner, &ctx.repo, inputs.number, &req)
                .await
                .context("merge request failed")?;
            Outcome::Merged
        }
    };

    // ---- 5. Best-effort label cleanup ------------------------------------
    if !inputs.filter_label.is_empty() {
        if let Err(e) = client
            .remove_label(&ctx.owner, &ctx.repo, inputs.number, &inputs.filter_label)
            .await
        {
            // Match the original behaviour: warn but do not fail the action.
            log.warning(&e.to_string());
        }
    }

    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github_client::{GitRef, Label};
    use crate::logger::CaptureLogger;
    use async_trait::async_trait;
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeClient {
        pr: Mutex<Option<PullRequest>>,
        get_pull_err: Mutex<Option<String>>,
        merge_calls: Mutex<Vec<(u64, MergeRequest)>>,
        update_ref_calls: Mutex<Vec<(String, String, bool)>>,
        remove_label_calls: Mutex<Vec<(u64, String)>>,
        remove_label_err: Mutex<Option<String>>,
    }

    impl FakeClient {
        fn with_pr(pr: PullRequest) -> Self {
            Self {
                pr: Mutex::new(Some(pr)),
                ..Default::default()
            }
        }
    }

    #[async_trait]
    impl GithubClient for FakeClient {
        async fn get_pull(&self, _o: &str, _r: &str, _n: u64) -> Result<PullRequest> {
            if let Some(e) = self.get_pull_err.lock().unwrap().clone() {
                return Err(anyhow::anyhow!(e));
            }
            Ok(self
                .pr
                .lock()
                .unwrap()
                .clone()
                .expect("test forgot to seed PR"))
        }

        async fn update_ref(
            &self,
            _o: &str,
            _r: &str,
            ref_: &str,
            sha: &str,
            force: bool,
        ) -> Result<()> {
            self.update_ref_calls
                .lock()
                .unwrap()
                .push((ref_.to_string(), sha.to_string(), force));
            Ok(())
        }

        async fn merge_pull(
            &self,
            _o: &str,
            _r: &str,
            number: u64,
            request: &MergeRequest,
        ) -> Result<()> {
            self.merge_calls
                .lock()
                .unwrap()
                .push((number, request.clone()));
            Ok(())
        }

        async fn remove_label(
            &self,
            _o: &str,
            _r: &str,
            issue_number: u64,
            label: &str,
        ) -> Result<()> {
            self.remove_label_calls
                .lock()
                .unwrap()
                .push((issue_number, label.to_string()));
            if let Some(e) = self.remove_label_err.lock().unwrap().clone() {
                return Err(anyhow::anyhow!(e));
            }
            Ok(())
        }
    }

    fn open_pr(labels: &[&str]) -> PullRequest {
        PullRequest {
            state: "open".to_string(),
            head: GitRef {
                ref_: "feature".to_string(),
                sha: "abc123".to_string(),
            },
            base: GitRef {
                ref_: "main".to_string(),
                sha: "main111".to_string(),
            },
            labels: labels
                .iter()
                .map(|n| Label {
                    name: (*n).to_string(),
                })
                .collect(),
        }
    }

    fn ctx() -> GithubContext {
        GithubContext {
            owner: "octo".into(),
            repo: "widget".into(),
            actor: "alice".into(),
            api_base_url: "https://api.github.com".into(),
        }
    }

    fn inputs(method: MergeMethod, regex: &str, label: &str) -> ActionInputs {
        ActionInputs {
            github_token: "t".into(),
            number: 7,
            merge_method: method,
            allowed_usernames_regex: regex.into(),
            filter_label: label.into(),
            merge_title: "".into(),
            merge_message: "".into(),
        }
    }

    #[tokio::test]
    async fn skips_when_actor_does_not_match() {
        let client = FakeClient::with_pr(open_pr(&[]));
        let mut log = CaptureLogger::new();
        let out = run(
            &client,
            &inputs(MergeMethod::Merge, "^bob$", ""),
            &ctx(),
            &mut log,
        )
        .await
        .unwrap();
        assert_eq!(out, Outcome::SkippedActor);
        assert!(log.contains("does not match"));
        // No GitHub calls should have happened.
        assert!(client.merge_calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn skips_when_pr_is_closed() {
        let mut pr = open_pr(&[]);
        pr.state = "closed".into();
        let client = FakeClient::with_pr(pr);
        let mut log = CaptureLogger::new();
        let out = run(
            &client,
            &inputs(MergeMethod::Merge, "^.*$", ""),
            &ctx(),
            &mut log,
        )
        .await
        .unwrap();
        assert_eq!(out, Outcome::SkippedClosed);
        assert!(log.contains("closed"));
        assert!(client.merge_calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn skips_when_required_label_is_missing() {
        let client = FakeClient::with_pr(open_pr(&["other"]));
        let mut log = CaptureLogger::new();
        let out = run(
            &client,
            &inputs(MergeMethod::Merge, "^.*$", "merge-it"),
            &ctx(),
            &mut log,
        )
        .await
        .unwrap();
        assert_eq!(out, Outcome::SkippedLabelMissing);
        assert!(log.contains("does not exist"));
    }

    #[tokio::test]
    async fn merges_when_label_present_and_removes_label() {
        let client = FakeClient::with_pr(open_pr(&["merge-it"]));
        let mut log = CaptureLogger::new();
        let out = run(
            &client,
            &inputs(MergeMethod::Squash, "^.*$", "merge-it"),
            &ctx(),
            &mut log,
        )
        .await
        .unwrap();
        assert_eq!(out, Outcome::Merged);

        let merges = client.merge_calls.lock().unwrap();
        assert_eq!(merges.len(), 1);
        assert_eq!(merges[0].0, 7);
        assert_eq!(merges[0].1.merge_method, "squash");
        assert_eq!(merges[0].1.sha, "abc123");

        let removed = client.remove_label_calls.lock().unwrap();
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0], (7, "merge-it".to_string()));
    }

    #[tokio::test]
    async fn merge_without_label_does_not_attempt_label_removal() {
        let client = FakeClient::with_pr(open_pr(&[]));
        let mut log = CaptureLogger::new();
        let out = run(
            &client,
            &inputs(MergeMethod::Merge, "^.*$", ""),
            &ctx(),
            &mut log,
        )
        .await
        .unwrap();
        assert_eq!(out, Outcome::Merged);
        assert!(client.remove_label_calls.lock().unwrap().is_empty());
        assert!(log.contains("Label check is disabled"));
    }

    #[tokio::test]
    async fn fast_forward_calls_update_ref_not_merge() {
        let client = FakeClient::with_pr(open_pr(&[]));
        let mut log = CaptureLogger::new();
        let out = run(
            &client,
            &inputs(MergeMethod::FastForward, "^.*$", ""),
            &ctx(),
            &mut log,
        )
        .await
        .unwrap();
        assert_eq!(out, Outcome::FastForwarded);

        let calls = client.update_ref_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0],
            ("heads/main".to_string(), "abc123".to_string(), false)
        );
        assert!(client.merge_calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn fast_forward_or_merge_succeeds_with_ff() {
        let client = FakeClient::with_pr(open_pr(&[]));
        let mut log = CaptureLogger::new();
        let out = run(
            &client,
            &inputs(MergeMethod::FastForwardOrMerge, "^.*$", ""),
            &ctx(),
            &mut log,
        )
        .await
        .unwrap();
        assert_eq!(out, Outcome::FastForwarded);

        let calls = client.update_ref_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert!(client.merge_calls.lock().unwrap().is_empty());
        assert!(log.contains("Fast-forward succeeded"));
    }

    #[tokio::test]
    async fn fast_forward_or_merge_falls_back_to_merge() {
        struct FailFfClient(FakeClient);
        #[async_trait]
        impl GithubClient for FailFfClient {
            async fn get_pull(&self, o: &str, r: &str, n: u64) -> Result<PullRequest> {
                self.0.get_pull(o, r, n).await
            }
            async fn update_ref(
                &self,
                _o: &str,
                _r: &str,
                _ref: &str,
                _sha: &str,
                _f: bool,
            ) -> Result<()> {
                Err(anyhow::anyhow!("not a fast-forward"))
            }
            async fn merge_pull(&self, o: &str, r: &str, n: u64, req: &MergeRequest) -> Result<()> {
                self.0.merge_pull(o, r, n, req).await
            }
            async fn remove_label(&self, o: &str, r: &str, n: u64, l: &str) -> Result<()> {
                self.0.remove_label(o, r, n, l).await
            }
        }

        let client = FailFfClient(FakeClient::with_pr(open_pr(&[])));
        let mut log = CaptureLogger::new();
        let out = run(
            &client,
            &inputs(MergeMethod::FastForwardOrMerge, "^.*$", ""),
            &ctx(),
            &mut log,
        )
        .await
        .unwrap();
        assert_eq!(out, Outcome::Merged);

        let merges = client.0.merge_calls.lock().unwrap();
        assert_eq!(merges.len(), 1);
        assert_eq!(merges[0].1.merge_method, "merge");
        assert!(log.contains("falling back to merge"));
    }

    #[tokio::test]
    async fn label_removal_failure_is_only_a_warning() {
        let client = FakeClient::with_pr(open_pr(&["merge-it"]));
        *client.remove_label_err.lock().unwrap() = Some("nope".into());
        let mut log = CaptureLogger::new();
        let out = run(
            &client,
            &inputs(MergeMethod::Merge, "^.*$", "merge-it"),
            &ctx(),
            &mut log,
        )
        .await
        .unwrap();
        // The merge itself still succeeds.
        assert_eq!(out, Outcome::Merged);
        assert!(log.contains("nope"));
    }

    #[tokio::test]
    async fn label_filter_supports_regex() {
        // The original code calls `new RegExp(filter_label)`, so a regex
        // pattern in `filter-label` must be honoured.
        let client = FakeClient::with_pr(open_pr(&["release-1.2.3"]));
        let mut log = CaptureLogger::new();
        let out = run(
            &client,
            &inputs(MergeMethod::Merge, "^.*$", "^release-"),
            &ctx(),
            &mut log,
        )
        .await
        .unwrap();
        assert_eq!(out, Outcome::Merged);
    }

    #[tokio::test]
    async fn invalid_username_regex_is_a_hard_error() {
        let client = FakeClient::with_pr(open_pr(&[]));
        let mut log = CaptureLogger::new();
        let err = run(
            &client,
            &inputs(MergeMethod::Merge, "(", ""),
            &ctx(),
            &mut log,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(err.contains("allowed-usernames-regex"));
    }

    #[tokio::test]
    async fn merge_failure_propagates() {
        struct FailingMerge(FakeClient);
        #[async_trait]
        impl GithubClient for FailingMerge {
            async fn get_pull(&self, o: &str, r: &str, n: u64) -> Result<PullRequest> {
                self.0.get_pull(o, r, n).await
            }
            async fn update_ref(
                &self,
                o: &str,
                r: &str,
                ref_: &str,
                sha: &str,
                f: bool,
            ) -> Result<()> {
                self.0.update_ref(o, r, ref_, sha, f).await
            }
            async fn merge_pull(
                &self,
                _o: &str,
                _r: &str,
                _n: u64,
                _req: &MergeRequest,
            ) -> Result<()> {
                Err(anyhow::anyhow!("API exploded"))
            }
            async fn remove_label(&self, o: &str, r: &str, n: u64, l: &str) -> Result<()> {
                self.0.remove_label(o, r, n, l).await
            }
        }

        let client = FailingMerge(FakeClient::with_pr(open_pr(&[])));
        let mut log = CaptureLogger::new();
        let err = run(
            &client,
            &inputs(MergeMethod::Merge, "^.*$", ""),
            &ctx(),
            &mut log,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(err.contains("merge request failed"));
    }
}
