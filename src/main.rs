use action_pull_request_merge::{
    action,
    logger::{self, StdoutLogger},
    ActionInputs, GiteaClient, GithubClient, GithubContext, OctocrabClient,
};
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    let mut log = StdoutLogger;
    let result = run(&mut log).await;
    logger::flush();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            // Mirror core.setFailed: emit an ::error:: and exit non-zero.
            use action_pull_request_merge::Logger as _;
            log.set_failed(&format!("{:#}", e));
            logger::flush();
            ExitCode::FAILURE
        }
    }
}

async fn run(log: &mut StdoutLogger) -> anyhow::Result<()> {
    let inputs = ActionInputs::from_env()?;
    let ctx = GithubContext::from_env()?;
    // Gitea's REST API differs on the merge endpoint and label-removal
    // flow, so route through a Gitea-aware client when running there.
    let client: Box<dyn GithubClient> = if ctx.is_gitea {
        use action_pull_request_merge::Logger as _;
        log.info("Detected Gitea Actions; using Gitea API client.");
        Box::new(GiteaClient::new(
            inputs.github_token.clone(),
            &ctx.api_base_url,
        )?)
    } else {
        Box::new(OctocrabClient::new(
            inputs.github_token.clone(),
            &ctx.api_base_url,
        )?)
    };
    action::run(client.as_ref(), &inputs, &ctx, log).await?;
    Ok(())
}
