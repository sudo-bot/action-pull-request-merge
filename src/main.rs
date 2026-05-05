use action_pull_request_merge::{
    action,
    logger::{self, StdoutLogger},
    pick_backend, ActionInputs, Backend, GiteaClient, GithubClient, GithubContext, OctocrabClient,
};
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    let mut log = StdoutLogger::default();
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
    let client: Box<dyn GithubClient> = match pick_backend(&ctx) {
        Backend::Gitea => {
            use action_pull_request_merge::Logger as _;
            log.info("Detected Gitea Actions; using Gitea API client.");
            Box::new(GiteaClient::new(
                inputs.github_token.clone(),
                &ctx.api_base_url,
            )?)
        }
        Backend::Github => Box::new(OctocrabClient::new(
            inputs.github_token.clone(),
            &ctx.api_base_url,
        )?),
    };
    action::run(client.as_ref(), &inputs, &ctx, log).await?;
    Ok(())
}
