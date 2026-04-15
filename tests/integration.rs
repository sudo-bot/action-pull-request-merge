//! End-to-end integration tests. They drive `action::run` through a fake
//! GitHub client while exercising realistic input/environment wiring.

use action_pull_request_merge::action::{run, Outcome};
use action_pull_request_merge::context::GithubContext;
use action_pull_request_merge::github_client::{
    GitRef, GithubClient, Label, MergeRequest, PullRequest,
};
use action_pull_request_merge::inputs::{ActionInputs, MapSource, MergeMethod};
use action_pull_request_merge::logger::CaptureLogger;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Mutex;

#[derive(Default)]
struct FakeClient {
    pr: Mutex<Option<PullRequest>>,
    merge_calls: Mutex<Vec<MergeRequest>>,
    update_ref_calls: Mutex<Vec<(String, String)>>,
    remove_label_calls: Mutex<Vec<String>>,
}

#[async_trait]
impl GithubClient for FakeClient {
    async fn get_pull(&self, _o: &str, _r: &str, _n: u64) -> Result<PullRequest> {
        Ok(self.pr.lock().unwrap().clone().unwrap())
    }
    async fn update_ref(
        &self,
        _o: &str,
        _r: &str,
        ref_: &str,
        sha: &str,
        _force: bool,
    ) -> Result<()> {
        self.update_ref_calls
            .lock()
            .unwrap()
            .push((ref_.to_string(), sha.to_string()));
        Ok(())
    }
    async fn merge_pull(&self, _o: &str, _r: &str, _n: u64, request: &MergeRequest) -> Result<()> {
        self.merge_calls.lock().unwrap().push(request.clone());
        Ok(())
    }
    async fn remove_label(&self, _o: &str, _r: &str, _n: u64, label: &str) -> Result<()> {
        self.remove_label_calls
            .lock()
            .unwrap()
            .push(label.to_string());
        Ok(())
    }
}

fn pr(labels: &[&str]) -> PullRequest {
    PullRequest {
        state: "open".into(),
        head: GitRef {
            ref_: "topic".into(),
            sha: "deadbeef".into(),
        },
        base: GitRef {
            ref_: "main".into(),
            sha: "cafef00d".into(),
        },
        labels: labels
            .iter()
            .map(|n| Label {
                name: (*n).to_string(),
            })
            .collect(),
    }
}

fn ctx(actor: &str) -> GithubContext {
    GithubContext {
        owner: "octo".into(),
        repo: "widget".into(),
        actor: actor.into(),
        api_base_url: "https://api.github.com".into(),
    }
}

#[tokio::test]
async fn end_to_end_inputs_parsing_drives_a_merge() {
    // Simulate the env-var-style inputs a runner would supply.
    let src = MapSource::new([
        ("github-token", "ghp_test"),
        ("number", "42"),
        ("merge-method", "merge"),
        ("allowed-usernames-regex", "^alice$"),
        ("filter-label", "merge-it"),
        ("merge-title", "title"),
        ("merge-message", "body"),
    ]);
    let inputs = ActionInputs::from_source(&src).unwrap();
    assert_eq!(inputs.number, 42);

    let client = FakeClient {
        pr: Mutex::new(Some(pr(&["merge-it"]))),
        ..Default::default()
    };
    let mut log = CaptureLogger::new();
    let out = run(&client, &inputs, &ctx("alice"), &mut log)
        .await
        .unwrap();
    assert_eq!(out, Outcome::Merged);

    let merges = client.merge_calls.lock().unwrap();
    assert_eq!(merges.len(), 1);
    assert_eq!(merges[0].merge_method, "merge");
    assert_eq!(merges[0].sha, "deadbeef");
    assert_eq!(merges[0].commit_title.as_deref(), Some("title"));
    assert_eq!(merges[0].commit_message.as_deref(), Some("body"));

    let removed = client.remove_label_calls.lock().unwrap();
    assert_eq!(removed.as_slice(), &["merge-it".to_string()]);
}

#[tokio::test]
async fn end_to_end_fast_forward_path() {
    let src = MapSource::new([
        ("github-token", "ghp_test"),
        ("number", "3"),
        ("merge-method", "fast-forward"),
    ]);
    let inputs = ActionInputs::from_source(&src).unwrap();
    assert_eq!(inputs.merge_method, MergeMethod::FastForward);

    let client = FakeClient {
        pr: Mutex::new(Some(pr(&[]))),
        ..Default::default()
    };
    let mut log = CaptureLogger::new();
    let out = run(&client, &inputs, &ctx("anyone"), &mut log)
        .await
        .unwrap();
    assert_eq!(out, Outcome::FastForwarded);

    let calls = client.update_ref_calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "heads/main");
    assert_eq!(calls[0].1, "deadbeef");
    assert!(client.merge_calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn end_to_end_disallowed_actor_is_skipped() {
    let src = MapSource::new([
        ("github-token", "t"),
        ("number", "1"),
        ("allowed-usernames-regex", "^bob$"),
    ]);
    let inputs = ActionInputs::from_source(&src).unwrap();
    let client = FakeClient {
        pr: Mutex::new(Some(pr(&[]))),
        ..Default::default()
    };
    let mut log = CaptureLogger::new();
    let out = run(&client, &inputs, &ctx("alice"), &mut log)
        .await
        .unwrap();
    assert_eq!(out, Outcome::SkippedActor);
    assert!(client.merge_calls.lock().unwrap().is_empty());
    assert!(client.update_ref_calls.lock().unwrap().is_empty());
}
