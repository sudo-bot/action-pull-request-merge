//! Read GitHub Actions runtime context from environment variables.
//!
//! GitHub Actions exposes the workflow context (repo, actor, event payload,
//! API URL, ...) through `GITHUB_*` environment variables. See:
//! <https://docs.github.com/en/actions/learn-github-actions/variables>

use anyhow::{anyhow, Context as _, Result};
use std::env;

#[derive(Debug, Clone)]
pub struct GithubContext {
    pub owner: String,
    pub repo: String,
    pub actor: String,
    pub api_base_url: String,
    /// True when running on Gitea Actions. Gitea's REST API is GitHub-shaped
    /// but differs on the merge endpoint and label-removal flow, so the
    /// client implementation must branch on this.
    pub is_gitea: bool,
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
        // The Gitea Actions runner sets GITEA_ACTIONS=true. If somebody runs
        // the binary outside Actions, the URL ending in `/api/v1` is also a
        // strong signal (GitHub uses no path, GHES uses `/api/v3`).
        let is_gitea = env::var("GITEA_ACTIONS")
            .map(|v| v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
            || api_base_url.trim_end_matches('/').ends_with("/api/v1");
        Ok(Self {
            owner: owner.to_string(),
            repo: repo.to_string(),
            actor,
            api_base_url,
            is_gitea,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// `cargo test` runs tests in parallel by default, but the process
    /// environment is shared. Serialise every env-touching test through this
    /// mutex so concurrent tests can't observe a half-mutated environment
    /// and fail with phantom "env var not found" errors.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Helper that runs a closure with controlled env vars and restores them
    /// afterwards. Tests touching the process environment must run serially
    /// inside this helper.
    fn with_env<F: FnOnce()>(pairs: &[(&str, Option<&str>)], f: F) {
        // Hold the lock for the entire set/run/restore window. If a previous
        // test panicked we still want isolation, so recover from poisoning.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
                ("GITEA_ACTIONS", None),
            ],
            || {
                let ctx = GithubContext::from_env().unwrap();
                assert_eq!(ctx.owner, "octo");
                assert_eq!(ctx.repo, "widget");
                assert_eq!(ctx.actor, "alice");
                assert_eq!(ctx.api_base_url, "https://api.github.com");
                assert!(!ctx.is_gitea);
            },
        );
    }

    #[test]
    fn detects_gitea_via_env_var() {
        with_env(
            &[
                ("GITHUB_REPOSITORY", Some("octo/widget")),
                ("GITHUB_ACTOR", Some("alice")),
                ("GITHUB_API_URL", Some("https://gitea.example.com/api/v1")),
                ("GITEA_ACTIONS", Some("true")),
            ],
            || {
                let ctx = GithubContext::from_env().unwrap();
                assert!(ctx.is_gitea);
            },
        );
    }

    #[test]
    fn detects_gitea_via_api_v1_path_even_without_env_var() {
        with_env(
            &[
                ("GITHUB_REPOSITORY", Some("octo/widget")),
                ("GITHUB_ACTOR", Some("alice")),
                ("GITHUB_API_URL", Some("https://gitea.example.com/api/v1")),
                ("GITEA_ACTIONS", None),
            ],
            || {
                let ctx = GithubContext::from_env().unwrap();
                assert!(ctx.is_gitea);
            },
        );
    }

    #[test]
    fn detects_gitea_with_trailing_slash_on_api_url() {
        // Some runners normalise URLs with a trailing slash; the suffix
        // check must tolerate it.
        with_env(
            &[
                ("GITHUB_REPOSITORY", Some("octo/widget")),
                ("GITHUB_ACTOR", Some("alice")),
                ("GITHUB_API_URL", Some("https://gitea.example.com/api/v1/")),
                ("GITEA_ACTIONS", None),
            ],
            || {
                let ctx = GithubContext::from_env().unwrap();
                assert!(ctx.is_gitea);
            },
        );
    }

    #[test]
    fn explicit_gitea_actions_false_disables_gitea_detection() {
        with_env(
            &[
                ("GITHUB_REPOSITORY", Some("octo/widget")),
                ("GITHUB_ACTOR", Some("alice")),
                // Plain github.com URL means we should not flip to Gitea
                // just because the env var exists with a non-truthy value.
                ("GITHUB_API_URL", Some("https://api.github.com")),
                ("GITEA_ACTIONS", Some("false")),
            ],
            || {
                let ctx = GithubContext::from_env().unwrap();
                assert!(!ctx.is_gitea);
            },
        );
    }

    #[test]
    fn gitea_actions_env_var_is_case_insensitive() {
        // The runner spec says the value is the literal `true`, but be
        // permissive — a `True` from a YAML expression must still work.
        with_env(
            &[
                ("GITHUB_REPOSITORY", Some("octo/widget")),
                ("GITHUB_ACTOR", Some("alice")),
                ("GITHUB_API_URL", None),
                ("GITEA_ACTIONS", Some("TRUE")),
            ],
            || {
                let ctx = GithubContext::from_env().unwrap();
                assert!(ctx.is_gitea);
            },
        );
    }

    #[test]
    fn rejects_malformed_repository() {
        with_env(
            &[
                ("GITHUB_REPOSITORY", Some("not-a-slash-pair")),
                ("GITHUB_ACTOR", Some("alice")),
                ("GITEA_ACTIONS", None),
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
                ("GITEA_ACTIONS", None),
            ],
            || {
                let ctx = GithubContext::from_env().unwrap();
                assert_eq!(ctx.api_base_url, "https://ghe.example.com/api/v3");
                // GHES is still GitHub, not Gitea.
                assert!(!ctx.is_gitea);
            },
        );
    }
}
