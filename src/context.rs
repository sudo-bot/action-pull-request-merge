//! Read GitHub Actions runtime context from environment variables.
//!
//! GitHub Actions exposes the workflow context (repo, actor, event payload,
//! API URL, ...) through `GITHUB_*` environment variables. See:
//! <https://docs.github.com/en/actions/learn-github-actions/variables>

use anyhow::{anyhow, Context as _, Result};
use std::env;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct GithubContext {
    pub owner: String,
    pub repo: String,
    pub actor: String,
    pub api_base_url: String,
}

impl GithubContext {
    pub fn from_env() -> Result<Self> {
        let repository =
            env::var("GITHUB_REPOSITORY").context("GITHUB_REPOSITORY env var is required")?;
        let (owner, repo) = repository.split_once('/').ok_or_else(|| {
            anyhow!(
                "GITHUB_REPOSITORY must look like 'owner/repo', got '{}'",
                repository
            )
        })?;
        let actor = env::var("GITHUB_ACTOR").context("GITHUB_ACTOR env var is required")?;
        let api_base_url =
            env::var("GITHUB_API_URL").unwrap_or_else(|_| "https://api.github.com".to_string());
        Ok(Self {
            owner: owner.to_string(),
            repo: repo.to_string(),
            actor,
            api_base_url,
        })
    }

    /// Optional path to the event payload JSON. Not used today but exposed
    /// for completeness in case future inputs default off it.
    pub fn event_path() -> Option<PathBuf> {
        env::var("GITHUB_EVENT_PATH").ok().map(PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper that runs a closure with controlled env vars and restores them
    /// afterwards. Tests touching the process environment must run serially
    /// inside this helper. We isolate by calling within a single test only.
    fn with_env<F: FnOnce()>(pairs: &[(&str, Option<&str>)], f: F) {
        let saved: Vec<_> = pairs
            .iter()
            .map(|(k, _)| (k.to_string(), env::var(k).ok()))
            .collect();
        for (k, v) in pairs {
            match v {
                Some(v) => env::set_var(k, v),
                None => env::remove_var(k),
            }
        }
        f();
        for (k, v) in saved {
            match v {
                Some(v) => env::set_var(&k, v),
                None => env::remove_var(&k),
            }
        }
    }

    #[test]
    fn parses_owner_repo_actor_and_default_api_url() {
        with_env(
            &[
                ("GITHUB_REPOSITORY", Some("octo/widget")),
                ("GITHUB_ACTOR", Some("alice")),
                ("GITHUB_API_URL", None),
            ],
            || {
                let ctx = GithubContext::from_env().unwrap();
                assert_eq!(ctx.owner, "octo");
                assert_eq!(ctx.repo, "widget");
                assert_eq!(ctx.actor, "alice");
                assert_eq!(ctx.api_base_url, "https://api.github.com");
            },
        );
    }

    #[test]
    fn rejects_malformed_repository() {
        with_env(
            &[
                ("GITHUB_REPOSITORY", Some("not-a-slash-pair")),
                ("GITHUB_ACTOR", Some("alice")),
            ],
            || {
                let err = GithubContext::from_env().unwrap_err().to_string();
                assert!(err.contains("owner/repo"));
            },
        );
    }

    #[test]
    fn honours_custom_api_url_for_ghes() {
        with_env(
            &[
                ("GITHUB_REPOSITORY", Some("octo/widget")),
                ("GITHUB_ACTOR", Some("alice")),
                ("GITHUB_API_URL", Some("https://ghe.example.com/api/v3")),
            ],
            || {
                let ctx = GithubContext::from_env().unwrap();
                assert_eq!(ctx.api_base_url, "https://ghe.example.com/api/v3");
            },
        );
    }
}
