//! Library crate for `action-pull-request-merge`.
//!
//! The binary in `src/main.rs` is a very thin wrapper around [`action::run`]
//! so that the entire workflow can be exercised with mocks in tests.

pub mod action;
pub mod context;
pub mod gitea_client;
pub mod github_client;
pub mod inputs;
pub mod logger;

pub use action::{run, Outcome};
pub use context::GithubContext;
pub use gitea_client::GiteaClient;
pub use github_client::{GithubClient, OctocrabClient};
pub use inputs::{ActionInputs, MergeMethod};
pub use logger::{Logger, StdoutLogger};

/// Which forge the action is talking to. Decided once from [`GithubContext`]
/// and used to pick the right [`GithubClient`] implementation at start-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Github,
    Gitea,
}

/// Pure mapping from runtime context to the chosen backend. Extracted so
/// the selection rule has its own unit test — `main.rs`'s tiny `match`
/// reduces to "given `Backend::Gitea`, build a `GiteaClient`", which is
/// hard to invert silently.
pub fn pick_backend(ctx: &GithubContext) -> Backend {
    if ctx.is_gitea {
        Backend::Gitea
    } else {
        Backend::Github
    }
}

#[cfg(test)]
mod backend_tests {
    use super::*;

    fn ctx_with(is_gitea: bool) -> GithubContext {
        GithubContext {
            owner: "o".into(),
            repo: "r".into(),
            actor: "a".into(),
            api_base_url: "https://x".into(),
            is_gitea,
        }
    }

    #[test]
    fn picks_gitea_when_flag_is_set() {
        assert_eq!(pick_backend(&ctx_with(true)), Backend::Gitea);
    }

    #[test]
    fn picks_github_when_flag_is_unset() {
        assert_eq!(pick_backend(&ctx_with(false)), Backend::Github);
    }
}
