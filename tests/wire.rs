//! HTTP wire-shape tests.
//!
//! The unit tests in `src/github_client.rs` and `src/gitea_client.rs` cover
//! the body serialization in isolation, but they don't prove that the
//! clients actually call the right *URL*, with the right HTTP *method*, or
//! send the *Authorization* header. A typo like `PUT` vs `POST`, or a
//! transposed path, would compile and pass every existing test.
//!
//! These tests stand up an in-process [`wiremock`] server, point each
//! client at it, and assert on the request shape.

use action_pull_request_merge::action::{run, Outcome};
use action_pull_request_merge::context::GithubContext;
use action_pull_request_merge::gitea_client::GiteaClient;
use action_pull_request_merge::github_client::{MergeRequest, OctocrabClient};
use action_pull_request_merge::inputs::ActionInputs;
use action_pull_request_merge::logger::CaptureLogger;
use action_pull_request_merge::{GithubClient, MergeMethod};
use serde_json::{json, Value};
use wiremock::matchers::{body_json, header_exists, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn pr_json() -> Value {
    json!({
        "state": "open",
        "head": { "ref": "topic", "sha": "abc123" },
        "base": { "ref": "main", "sha": "def456" },
        "labels": [
            { "id": 11, "name": "merge-it" },
            { "id": 22, "name": "release" },
        ],
    })
}

// ---------------------------------------------------------------------------
// OctocrabClient (GitHub) wire shape
// ---------------------------------------------------------------------------

#[tokio::test]
async fn github_get_pull_uses_get_on_repos_pulls_path_with_auth_header() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/octo/widget/pulls/7"))
        .and(header_exists("authorization"))
        .respond_with(ResponseTemplate::new(200).set_body_json(pr_json()))
        .expect(1)
        .mount(&server)
        .await;

    let client = OctocrabClient::new("token-abc".into(), &server.uri()).unwrap();
    let pr = client.get_pull("octo", "widget", 7).await.unwrap();
    assert_eq!(pr.head.sha, "abc123");
    assert_eq!(pr.labels.len(), 2);
}

#[tokio::test]
async fn github_merge_pull_uses_put_with_merge_method_body() {
    // GitHub takes PUT (not POST) on /pulls/{n}/merge with the GitHub-style
    // `merge_method` / `sha` body.
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/repos/octo/widget/pulls/7/merge"))
        .and(header_exists("authorization"))
        .and(body_json(json!({
            "merge_method": "squash",
            "sha": "abc123",
            "commit_title": "T",
            "commit_message": "M",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "sha": "abc123",
            "merged": true,
            "message": "Pull Request successfully merged",
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = OctocrabClient::new("token-abc".into(), &server.uri()).unwrap();
    let req = MergeRequest::from_inputs(MergeMethod::Squash, "abc123", "T", "M");
    client.merge_pull("octo", "widget", 7, &req).await.unwrap();
}

#[tokio::test]
async fn github_fast_forward_uses_patch_on_git_refs_path() {
    // GitHub's fast-forward goes through the git/refs API: PATCH the
    // base branch ref to the new SHA with force:false.
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/repos/octo/widget/git/refs/heads/main"))
        .and(header_exists("authorization"))
        .and(body_json(json!({ "sha": "abc123", "force": false })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ref": "refs/heads/main",
            "object": { "sha": "abc123" },
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = OctocrabClient::new("token-abc".into(), &server.uri()).unwrap();
    client
        .fast_forward("octo", "widget", 7, "main", "abc123")
        .await
        .unwrap();
}

#[tokio::test]
async fn github_remove_label_uses_delete_with_encoded_name_in_path() {
    // The label name carries URL-meta characters that have to be percent-
    // encoded (e.g. `+`, `?`, `/`); the request must hit the encoded path.
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path(
            "/repos/octo/widget/issues/7/labels/needs%20review%2Furgent%3F",
        ))
        .and(header_exists("authorization"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&server)
        .await;

    let client = OctocrabClient::new("token-abc".into(), &server.uri()).unwrap();
    client
        .remove_label("octo", "widget", 7, "needs review/urgent?")
        .await
        .unwrap();
}

// ---------------------------------------------------------------------------
// GiteaClient wire shape
// ---------------------------------------------------------------------------

#[tokio::test]
async fn gitea_merge_pull_uses_post_with_camelcase_body_and_accepts_empty_response() {
    // The two things the unit tests can't see: that we send `POST` (not
    // `PUT`) and that the empty 200 body Gitea returns is treated as
    // success rather than a deserialisation error.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/repos/octo/widget/pulls/7/merge"))
        .and(header_exists("authorization"))
        .and(body_json(json!({
            "Do": "squash",
            "MergeTitleField": "T",
            "MergeMessageField": "M",
            "head_commit_id": "abc123",
        })))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let client = GiteaClient::new("token-abc".into(), &server.uri()).unwrap();
    let req = MergeRequest::from_inputs(MergeMethod::Squash, "abc123", "T", "M");
    client.merge_pull("octo", "widget", 7, &req).await.unwrap();
}

#[tokio::test]
async fn gitea_merge_pull_surfaces_4xx_as_error() {
    // A 422 from Gitea (e.g. PR not mergeable) must propagate as a
    // failure with the status code visible in the message — otherwise
    // a non-mergeable PR would be silently treated as merged.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/repos/octo/widget/pulls/7/merge"))
        .respond_with(ResponseTemplate::new(422).set_body_json(json!({
            "message": "Pull request is not mergeable",
        })))
        .mount(&server)
        .await;

    let client = GiteaClient::new("token-abc".into(), &server.uri()).unwrap();
    let req = MergeRequest::from_inputs(MergeMethod::Merge, "abc", "", "");
    let err = client
        .merge_pull("octo", "widget", 7, &req)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("422"), "expected 422 in error: {}", err);
    // The error must name the URL the action actually called, otherwise a
    // 405 / 404 from a misconfigured Gitea instance is impossible to debug.
    assert!(
        err.contains("/repos/octo/widget/pulls/7/merge"),
        "expected URL path in error for diagnostics: {}",
        err
    );
    // Gitea's response body contains a `message` field with the actual
    // reason (e.g. "merge style is not allowed"). It must surface in the
    // error so the user doesn't need to guess.
    assert!(
        err.contains("Pull request is not mergeable"),
        "expected Gitea's error message in output: {}",
        err
    );
}

#[tokio::test]
async fn gitea_remove_label_does_get_then_delete_by_id() {
    // Gitea's DELETE wants the numeric label id. The client must first
    // GET the issue's labels, find the id by name, then DELETE — and
    // it must not call DELETE with the *name* in the URL.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/octo/widget/issues/7/labels"))
        .and(header_exists("authorization"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "id": 11, "name": "merge-it" },
            { "id": 22, "name": "release" },
        ])))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/repos/octo/widget/issues/7/labels/22"))
        .and(header_exists("authorization"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    // Guard rail: a DELETE by *name* would be a regression. Make any such
    // request fail loudly so the test catches it instead of hanging on
    // silently-handled 404s.
    Mock::given(method("DELETE"))
        .and(path("/repos/octo/widget/issues/7/labels/release"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;

    let client = GiteaClient::new("token-abc".into(), &server.uri()).unwrap();
    client
        .remove_label("octo", "widget", 7, "release")
        .await
        .unwrap();
}

#[tokio::test]
async fn gitea_fast_forward_uses_post_on_merge_endpoint_with_fast_forward_only_do() {
    // Gitea's `git/refs` API is read-only — the fast-forward must go
    // through the pull-request merge endpoint with `Do: fast-forward-only`,
    // not the PATCH on `git/refs` that GitHub uses (Gitea returns 405).
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/repos/octo/widget/pulls/7/merge"))
        .and(header_exists("authorization"))
        .and(body_json(json!({
            "Do": "fast-forward-only",
            "head_commit_id": "abc123",
        })))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let client = GiteaClient::new("token-abc".into(), &server.uri()).unwrap();
    client
        .fast_forward("octo", "widget", 7, "main", "abc123")
        .await
        .unwrap();
}

#[tokio::test]
async fn gitea_fast_forward_surfaces_4xx_with_url_in_error() {
    // When Gitea rejects the fast-forward (e.g. the merge style is
    // disabled in repo settings), the error must include the status code,
    // the path, AND Gitea's error message from the response body.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/repos/octo/widget/pulls/7/merge"))
        .respond_with(ResponseTemplate::new(405).set_body_json(json!({
            "message": "merge style 'fast-forward-only' is not allowed for this repository",
        })))
        .mount(&server)
        .await;

    let client = GiteaClient::new("token-abc".into(), &server.uri()).unwrap();
    let err = client
        .fast_forward("octo", "widget", 7, "main", "abc123")
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("405"), "expected 405 in error: {}", err);
    assert!(
        err.contains("/repos/octo/widget/pulls/7/merge"),
        "expected URL path in error for diagnostics: {}",
        err
    );
    assert!(
        err.contains("is not allowed for this repository"),
        "expected Gitea's error message in output: {}",
        err
    );
}

#[tokio::test]
async fn fast_forward_or_merge_on_older_gitea_falls_back_to_plain_merge() {
    // End-to-end check that the action's `fast-forward_or_merge` mode works
    // on Gitea < 1.22 (which doesn't recognise `Do: "fast-forward-only"`).
    //
    // Newer Gitea: 1 HTTP call (FF succeeds, covered by another test).
    // Older Gitea: 2 HTTP calls — FF returns 422 with an "unknown Do" error,
    // and the action falls back to a plain `Do: "merge"` POST.
    //
    // wiremock matches mocks in mount-order; the FF-only mock (with the
    // specific body matcher) goes first so it captures the FF attempt,
    // and the broader "Do:merge" mock captures the fallback.
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repos/octo/widget/pulls/7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "state": "open",
            "head": { "ref": "topic", "sha": "abc123" },
            "base": { "ref": "main", "sha": "def456" },
            "labels": [],
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/repos/octo/widget/pulls/7/merge"))
        .and(body_json(json!({
            "Do": "fast-forward-only",
            "head_commit_id": "abc123",
        })))
        .respond_with(ResponseTemplate::new(422).set_body_json(json!({
            "message": "Do: must be one of [merge rebase rebase-merge squash manually-merged head-only base-only]",
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/repos/octo/widget/pulls/7/merge"))
        .and(body_json(json!({
            "Do": "merge",
            "head_commit_id": "abc123",
        })))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let client = GiteaClient::new("token-abc".into(), &server.uri()).unwrap();
    let inputs = ActionInputs {
        github_token: "token-abc".into(),
        number: 7,
        merge_method: MergeMethod::FastForwardOrMerge,
        allowed_usernames_regex: ".*".into(),
        filter_label: "".into(),
        merge_title: "".into(),
        merge_message: "".into(),
    };
    let ctx = GithubContext {
        owner: "octo".into(),
        repo: "widget".into(),
        actor: "alice".into(),
        api_base_url: server.uri(),
        is_gitea: true,
    };
    let mut log = CaptureLogger::new();
    let outcome = run(&client, &inputs, &ctx, &mut log).await.unwrap();

    assert_eq!(outcome, Outcome::Merged);
    assert!(
        log.contains("falling back to merge"),
        "expected the warning that drives the fallback, got: {:?}",
        log
    );
    // wiremock's `.expect(N)` assertions fire on Drop, so MockServer's drop
    // here will panic if either mock was hit the wrong number of times.
}

#[tokio::test]
async fn gitea_get_pull_uses_get_on_repos_pulls_path() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/octo/widget/pulls/7"))
        .and(header_exists("authorization"))
        .respond_with(ResponseTemplate::new(200).set_body_json(pr_json()))
        .expect(1)
        .mount(&server)
        .await;

    let client = GiteaClient::new("token-abc".into(), &server.uri()).unwrap();
    let pr = client.get_pull("octo", "widget", 7).await.unwrap();
    assert_eq!(pr.head.sha, "abc123");
    assert_eq!(pr.labels.len(), 2);
}
