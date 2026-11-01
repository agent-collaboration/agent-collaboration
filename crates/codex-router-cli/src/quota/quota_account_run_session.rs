//! The CLI owns accepted mutations across every render-loop exit.
use agent_router_quota_tui::{
    QuotaAccountCommandQueue, QuotaAccountCommands, QuotaAccountCompletion, QuotaOutcome,
    quota_account_command_runner,
};
use std::{future::Future, io};

pub(crate) struct QuotaAccountSessionResult {
    pub(crate) outcome: io::Result<QuotaOutcome>,
    pub(crate) completions: Vec<QuotaAccountCompletion>,
}

pub(crate) async fn run_quota_account_session<TView, TViewFuture>(
    commands: QuotaAccountCommands,
    run_view: TView,
) -> io::Result<QuotaAccountSessionResult>
where
    TView: FnOnce(QuotaAccountCommandQueue) -> TViewFuture,
    TViewFuture: Future<Output = io::Result<QuotaOutcome>>,
{
    let (queue, runner) = quota_account_command_runner(commands);
    let task = tokio::spawn(runner);
    let outcome = run_view(queue.clone()).await;
    queue.close();
    let completions = task
        .await
        .map_err(|error| io::Error::other(format!("quota account runner failed: {error}")))?;
    Ok(QuotaAccountSessionResult {
        outcome,
        completions,
    })
}
