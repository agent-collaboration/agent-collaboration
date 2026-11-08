//! A caller that waits for an answer a store will commit: it reads, and when the answer is
//! not there yet it sleeps until the store's next commit or its own deadline, never on a tick.
use std::future::Future;
use tokio::sync::watch;

/// What ended one wait.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommitWaitEnd {
    /// The store committed a change; read again.
    Committed,
    /// The caller's timer elapsed.
    TimerElapsed,
    /// The store is gone and will never commit again.
    StoreClosed,
}

/// Sleeps until `commits` changes or `timer` elapses. Mark `commits` seen before the read
/// that found nothing, and never after it, so a commit that lands after that read still ends
/// this wait.
pub async fn wait_for_commit_or_timer(
    commits: &mut watch::Receiver<u64>,
    timer: impl Future<Output = ()>,
) -> CommitWaitEnd {
    tokio::select! {
        biased;
        changed = commits.changed() => match changed {
            Ok(()) => CommitWaitEnd::Committed,
            Err(_) => CommitWaitEnd::StoreClosed,
        },
        () = timer => CommitWaitEnd::TimerElapsed,
    }
}
