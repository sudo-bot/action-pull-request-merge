//! Library crate for `action-pull-request-merge`.
//!
//! The binary in `src/main.rs` is a very thin wrapper around [`action::run`]
//! so that the entire workflow can be exercised with mocks in tests.

pub mod action;
pub mod context;
pub mod github_client;
pub mod inputs;
pub mod logger;

pub use action::{run, Outcome};
pub use context::GithubContext;
pub use github_client::{GithubClient, OctocrabClient};
pub use inputs::{ActionInputs, MergeMethod};
pub use logger::{Logger, StdoutLogger};
