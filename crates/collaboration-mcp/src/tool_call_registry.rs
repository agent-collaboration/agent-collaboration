//! The tool calls one listener is running, and the two ways a call ends before it finishes:
//! its caller disconnects, or the listener's shutdown aborts it.
//!
//! rmcp runs each request's handler in a task of its own and, when the caller disconnects,
//! only cancels that request's token. A handler that does not watch the token keeps running
//! after its caller is gone, still holding the Router's stores, while the listener's admission
//! slot has already been released. So every call runs through the registry: a caller's
//! disconnect drops its call where it stands, and shutdown first lets calls finish on their
//! own, then aborts the rest by dropping them, and joins them all before the listener reports
//! that it stopped. Work a call handed to an owner of its own (a provider operation, a
//! message's delivery) is not dropped with it.
use std::future::Future;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

/// Why a call ended before it finished.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CallEndedEarly {
    /// The caller disconnected before the call answered.
    CallerDisconnected,
    /// The listener's shutdown grace period ended while the call was still running.
    AbortedAtShutdown,
}

#[derive(Clone, Debug)]
pub(crate) struct ToolCallRegistry {
    calls: TaskTracker,
    abort: CancellationToken,
    /// Cancelled when the listener starts stopping. rmcp then cancels every request's token
    /// too, which is not a disconnect: the shutdown grace period owns the calls from there.
    listener_stopping: CancellationToken,
    #[cfg(any(test, feature = "test-support"))]
    activity: tokio::sync::watch::Sender<()>,
    #[cfg(test)]
    shutdown_cancellation_observed: CancellationToken,
}

#[cfg(any(test, feature = "test-support"))]
struct RegistryActivityNotification(tokio::sync::watch::Sender<()>);

#[cfg(any(test, feature = "test-support"))]
impl Drop for RegistryActivityNotification {
    fn drop(&mut self) {
        self.0.send_replace(());
    }
}

impl ToolCallRegistry {
    pub(crate) fn new(listener_stopping: CancellationToken) -> Self {
        Self {
            calls: TaskTracker::new(),
            abort: CancellationToken::new(),
            listener_stopping,
            #[cfg(any(test, feature = "test-support"))]
            activity: tokio::sync::watch::channel(()).0,
            #[cfg(test)]
            shutdown_cancellation_observed: CancellationToken::new(),
        }
    }

    /// Runs one call. If its caller disconnects (`caller` is cancelled while the listener is
    /// not stopping), or shutdown aborts the listener's calls, the call is dropped where it
    /// stands and `ended_early` answers in its place; nobody reads a disconnected answer.
    pub(crate) async fn run<TOutput>(
        &self,
        call: impl Future<Output = TOutput>,
        caller: CancellationToken,
        ended_early: impl FnOnce(CallEndedEarly) -> TOutput,
    ) -> TOutput {
        let abort = self.abort.clone();
        let listener_stopping = self.listener_stopping.clone();
        #[cfg(test)]
        let shutdown_cancellation_observed = self.shutdown_cancellation_observed.clone();
        // Drop after the tracked future, including when its handler is aborted.
        #[cfg(any(test, feature = "test-support"))]
        let _activity_on_drop = RegistryActivityNotification(self.activity.clone());
        let tracked = self.calls.track_future(async move {
            let caller_disconnected = async {
                caller.cancelled().await;
                if listener_stopping.is_cancelled() {
                    #[cfg(test)]
                    shutdown_cancellation_observed.cancel();
                    std::future::pending::<()>().await;
                }
            };
            tokio::select! {
                biased;
                () = abort.cancelled() => ended_early(CallEndedEarly::AbortedAtShutdown),
                () = caller_disconnected => ended_early(CallEndedEarly::CallerDisconnected),
                output = call => output,
            }
        });
        #[cfg(any(test, feature = "test-support"))]
        self.activity.send_replace(());
        tracked.await
    }

    /// Drops every running call and every call that starts from now on.
    pub(crate) fn abort_all(&self) {
        self.abort.cancel();
    }

    /// Calls running now.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn running(&self) -> usize {
        self.calls.len()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) async fn await_running(&self, expected: usize) {
        let mut activity = self.activity.subscribe();
        loop {
            activity.borrow_and_update();
            if self.running() == expected {
                return;
            }
            activity
                .changed()
                .await
                .expect("registry observation remains alive");
        }
    }

    /// Resolves once no call is running. Calls that start later are still waited for.
    pub(crate) async fn ended(&self) {
        self.calls.close();
        self.calls.wait().await;
    }
}

#[cfg(test)]
mod tests {
    use super::{CallEndedEarly, ToolCallRegistry};
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;

    fn never_finishes(
        registry: &ToolCallRegistry,
        caller: &CancellationToken,
    ) -> tokio::task::JoinHandle<Result<&'static str, CallEndedEarly>> {
        let registry = registry.clone();
        let caller = caller.clone();
        tokio::spawn(async move { registry.run(std::future::pending(), caller, Err).await })
    }

    #[tokio::test]
    async fn aborting_drops_a_call_that_ignores_cancellation_and_ends_the_registry() {
        // Arrange: a call that never finishes on its own.
        let registry = ToolCallRegistry::new(CancellationToken::new());
        let running = never_finishes(&registry, &CancellationToken::new());
        tokio::time::timeout(Duration::from_secs(1), registry.await_running(1))
            .await
            .unwrap();

        // Act
        registry.abort_all();
        let answer = tokio::time::timeout(Duration::from_secs(1), running).await;
        let ended = tokio::time::timeout(Duration::from_secs(1), registry.ended()).await;

        // Assert
        assert_eq!(
            answer.ok().and_then(Result::ok),
            Some(Err(CallEndedEarly::AbortedAtShutdown))
        );
        assert!(ended.is_ok(), "the registry still had a running call");
    }

    #[tokio::test]
    async fn a_disconnected_caller_drops_a_call_that_ignores_cancellation() {
        // Arrange
        let registry = ToolCallRegistry::new(CancellationToken::new());
        let caller = CancellationToken::new();
        let running = never_finishes(&registry, &caller);
        tokio::time::timeout(Duration::from_secs(1), registry.await_running(1))
            .await
            .unwrap();

        // Act
        caller.cancel();
        let answer = tokio::time::timeout(Duration::from_secs(1), running).await;

        // Assert
        assert_eq!(
            answer.ok().and_then(Result::ok),
            Some(Err(CallEndedEarly::CallerDisconnected))
        );
        assert_eq!(registry.running(), 0);
    }

    #[tokio::test]
    async fn a_request_token_cancelled_by_shutdown_leaves_the_call_to_its_grace_period() {
        // Arrange: the listener is stopping, so rmcp has cancelled every request's token.
        let listener_stopping = CancellationToken::new();
        let registry = ToolCallRegistry::new(listener_stopping.clone());
        let caller = CancellationToken::new();
        let running = never_finishes(&registry, &caller);
        tokio::time::timeout(Duration::from_secs(1), registry.await_running(1))
            .await
            .unwrap();

        // Act
        listener_stopping.cancel();
        caller.cancel();
        tokio::time::timeout(
            Duration::from_secs(1),
            registry.shutdown_cancellation_observed.cancelled(),
        )
        .await
        .expect("the running call observes listener shutdown before checking grace");
        let mut during_grace = std::pin::pin!(registry.ended());
        let still_running = futures_util::poll!(&mut during_grace).is_pending();
        registry.abort_all();
        let answer = tokio::time::timeout(Duration::from_secs(1), running).await;

        // Assert
        assert!(still_running, "the call ended before its grace period");
        assert_eq!(
            answer.ok().and_then(Result::ok),
            Some(Err(CallEndedEarly::AbortedAtShutdown))
        );
    }

    #[tokio::test]
    async fn an_aborted_handler_notifies_a_waiter_for_the_registry_to_be_empty() {
        let registry = ToolCallRegistry::new(CancellationToken::new());
        let running = never_finishes(&registry, &CancellationToken::new());
        tokio::time::timeout(Duration::from_secs(1), registry.await_running(1))
            .await
            .unwrap();
        let mut empty = std::pin::pin!(registry.await_running(0));
        assert!(futures_util::poll!(empty.as_mut()).is_pending());

        running.abort();
        assert!(running.await.unwrap_err().is_cancelled());

        tokio::time::timeout(Duration::from_secs(1), empty)
            .await
            .expect("dropping an admitted handler must notify the empty-registry waiter");
    }

    #[tokio::test]
    async fn a_finished_call_answers_normally_and_leaves_nothing_running() {
        let registry = ToolCallRegistry::new(CancellationToken::new());
        let answer = registry
            .run(
                async { "done" },
                CancellationToken::new(),
                |_| "ended early",
            )
            .await;
        assert_eq!(answer, "done");
        let ended = tokio::time::timeout(Duration::from_secs(1), registry.ended()).await;
        assert!(ended.is_ok());
    }
}
