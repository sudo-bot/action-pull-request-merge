use action_pull_request_merge::{
    action,
    logger::{self, StdoutLogger},
    ActionInputs, GithubContext, OctocrabClient,
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
    let client = OctocrabClient::new(inputs.github_token.clone(), &ctx.api_base_url)?;
    action::run(&client, &inputs, &ctx, log).await?;
    Ok(())
}
