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

use action_pull_request_merge::gitea_client::GiteaClient;
use action_pull_request_merge::github_client::{MergeRequest, OctocrabClient};
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
async fn github_update_ref_uses_patch_on_git_refs_path() {
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
        .update_ref("octo", "widget", "heads/main", "abc123", false)
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
async fn gitea_update_ref_uses_patch_with_sha_force_body() {
    // Wire-compatible with GitHub on the spec, but verify it explicitly
    // — a regression that used POST/PUT here would still parse.
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

    let client = GiteaClient::new("token-abc".into(), &server.uri()).unwrap();
    client
        .update_ref("octo", "widget", "heads/main", "abc123", false)
        .await
        .unwrap();
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
