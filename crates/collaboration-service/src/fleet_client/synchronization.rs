//! Each caller receives the outcome of a fresh run, including after another holder.
use super::{FleetClient, FleetClientError};
use std::{collections::VecDeque, sync::Arc};
use tokio::sync::{Mutex, oneshot};

type SharedOutcome = Result<(), Arc<FleetClientError>>;

struct WaitingSynchronization {
    generation: u64,
    completion: oneshot::Sender<SharedOutcome>,
}

#[derive(Default)]
struct SynchronizationState {
    requested: u64,
    completed: u64,
    waiting: VecDeque<WaitingSynchronization>,
}

#[derive(Default)]
pub(super) struct SynchronizationRequests(Mutex<SynchronizationState>);

impl SynchronizationRequests {
    async fn request(&self) -> Result<oneshot::Receiver<SharedOutcome>, FleetClientError> {
        let mut state = self.0.lock().await;
        let generation = state
            .requested
            .checked_add(1)
            .ok_or(FleetClientError::SynchronizationGenerationExhausted)?;
        let (completion, waiting) = oneshot::channel();
        state.requested = generation;
        state.waiting.push_back(WaitingSynchronization {
            generation,
            completion,
        });
        Ok(waiting)
    }

    async fn pending(&self) -> Option<u64> {
        let state = self.0.lock().await;
        (state.requested > state.completed).then_some(state.requested)
    }

    async fn finish(&self, through: u64, outcome: &SharedOutcome) {
        let mut state = self.0.lock().await;
        if outcome.is_ok() {
            state.completed = through;
        }
        while state
            .waiting
            .front()
            .is_some_and(|waiting| waiting.generation <= through)
        {
            if let Some(waiting) = state.waiting.pop_front() {
                // A cancelled caller needs no result; its requested read still happened.
                let _ = waiting.completion.send(outcome.clone());
            }
        }
    }
}

impl FleetClient {
    /// Welcome, feed hints and the connected refresh timer all enter this owner.
    /// Completes only after a synchronization started after this call succeeds or fails.
    pub async fn synchronize(&self) -> Result<(), FleetClientError> {
        let mut completion = self.synchronization.request().await?;
        let outcome = tokio::select! {
            outcome = &mut completion => outcome,
            guard = self.admission_mutex.lock() => {
                // Another holder may already have delivered this generation's result.
                match completion.try_recv() {
                    Ok(outcome) => {
                        drop(guard);
                        return unwrap_outcome(outcome);
                    }
                    Err(oneshot::error::TryRecvError::Closed) => {
                        return Err(FleetClientError::Unavailable);
                    }
                    Err(oneshot::error::TryRecvError::Empty) => {}
                }
                let _ = self.synchronize_pending_batch_admitted().await;
                drop(guard);
                completion.await
            }
        };
        unwrap_outcome(outcome.map_err(|_| FleetClientError::Unavailable)?)
    }

    pub(super) async fn synchronize_pending_batch_admitted(&self) -> SharedOutcome {
        if let Some(generation) = self.synchronization.pending().await {
            // Snapshot before starting: a request arriving during this run needs the next run.
            let outcome = self.synchronize_admitted().await.map_err(Arc::new);
            self.synchronization.finish(generation, &outcome).await;
            return outcome;
        }
        Ok(())
    }
}

fn unwrap_outcome(outcome: SharedOutcome) -> Result<(), FleetClientError> {
    outcome.map_err(|error| {
        // Keep existing typed errors for a single caller; concurrent callers share the source.
        Arc::try_unwrap(error).unwrap_or_else(FleetClientError::Synchronization)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[allow(clippy::panic_in_result_fn)]
    async fn a_later_success_does_not_replace_an_earlier_waiters_error()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let requests = SynchronizationRequests::default();
        let failed = requests.request().await?;
        let first = requests.pending().await.ok_or("first generation missing")?;
        requests
            .finish(first, &Err(Arc::new(FleetClientError::Unavailable)))
            .await;
        let successful = requests.request().await?;
        let second = requests
            .pending()
            .await
            .ok_or("second generation missing")?;
        assert!(second > first);
        requests.finish(second, &Ok(())).await;
        assert!(successful.await?.is_ok());
        assert!(matches!(
            unwrap_outcome(failed.await?),
            Err(FleetClientError::Unavailable)
        ));
        assert!(requests.pending().await.is_none());
        Ok(())
    }
}
