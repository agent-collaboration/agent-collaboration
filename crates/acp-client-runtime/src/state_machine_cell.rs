//! One item's state cell (D1 state machines; design note §11.1, §12.1, §13.3).
//!
//! The cell holds the item's current state, a sequence number and an ordered
//! outbox behind the item's own lock, which is never held across an await. A
//! transition is the machine's pure `transition`; a winning one appends its
//! effects, and one runner per item applies them in order through the
//! connection's task tracker. Effects can apply events to the same cell; the
//! lock is free while they run. After shutdown's barrier no runner starts.

use futures_util::future::BoxFuture;
use state_machine_core::{StateMachine, TransitionError};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio_util::task::TaskTracker;

/// The tasks one provider connection owns, and its shutdown barrier.
#[derive(Default)]
pub struct CellRuntime {
    tracker: TaskTracker,
    no_new_runner: AtomicBool,
}

impl CellRuntime {
    /// Shutdown step 4 (design note §13.3): wait for every runner and
    /// completion task, then bar new runners.
    pub async fn drain(&self) {
        self.tracker.close();
        self.tracker.wait().await;
        self.no_new_runner.store(true, Ordering::SeqCst);
    }

    /// Spawns a task the connection owns, such as a request's completion.
    /// Refused after the barrier, returning false.
    pub fn spawn_tracked(
        &self,
        task: impl std::future::Future<Output = ()> + Send + 'static,
    ) -> bool {
        if self.no_new_runner.load(Ordering::SeqCst) {
            tracing::warn!("a connection task was refused after shutdown drained");
            return false;
        }
        drop(self.tracker.spawn(task));
        true
    }
}

/// Applies one machine's effects. An effect may apply events to `cell`.
pub trait CellEffects<M: StateMachine>: Send + Sync + 'static {
    fn apply_effect<'a>(
        &'a self,
        cell: &'a Arc<MachineCell<M>>,
        effect: M::Effect,
    ) -> BoxFuture<'a, ()>;
}

struct CellInner<M: StateMachine> {
    state: M::State,
    sequence: u64,
    outbox: VecDeque<(u64, M::Effect)>,
    runner_active: bool,
}

pub struct MachineCell<M: StateMachine> {
    inner: Mutex<CellInner<M>>,
    effects: Arc<dyn CellEffects<M>>,
    runtime: Arc<CellRuntime>,
}

impl<M> MachineCell<M>
where
    M: StateMachine + 'static,
    M::State: Send,
    M::Effect: Send + 'static,
{
    pub fn new(
        state: M::State,
        effects: Arc<dyn CellEffects<M>>,
        runtime: Arc<CellRuntime>,
    ) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(CellInner {
                state,
                sequence: 0,
                outbox: VecDeque::new(),
                runner_active: false,
            }),
            effects,
            runtime,
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, CellInner<M>> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The item's current state.
    pub fn current(&self) -> M::State {
        self.lock().state.clone()
    }

    /// Reads the current state under the cell's lock, for an effect's guard.
    pub fn inspect<R>(&self, read: impl FnOnce(&M::State) -> R) -> R {
        read(&self.lock().state)
    }

    /// Applies one event: the pure transition, then (on success) the new
    /// state, its effects appended in order, and a runner if none is active.
    /// A refusal changes nothing.
    pub fn apply(self: &Arc<Self>, event: M::Event) -> Result<M::State, TransitionError<M::Error>> {
        let mut inner = self.lock();
        let (next, effects) = M::transition(&inner.state, event)?;
        inner.state = next.clone();
        if !effects.is_empty() {
            inner.sequence = inner.sequence.wrapping_add(1);
            let sequence = inner.sequence;
            inner
                .outbox
                .extend(effects.into_iter().map(|effect| (sequence, effect)));
            if !inner.runner_active {
                let cell = Arc::clone(self);
                if self
                    .runtime
                    .spawn_tracked(async move { cell.run_effects().await })
                {
                    inner.runner_active = true;
                }
            }
        }
        Ok(next)
    }

    async fn run_effects(self: Arc<Self>) {
        loop {
            let next = {
                let mut inner = self.lock();
                match inner.outbox.pop_front() {
                    Some((_, effect)) => effect,
                    None => {
                        inner.runner_active = false;
                        return;
                    }
                }
            };
            self.effects.apply_effect(&self, next).await;
        }
    }

    /// Effects not yet applied, for tests and the shutdown report.
    pub fn pending_effects(&self) -> usize {
        self.lock().outbox.len()
    }
}

#[cfg(test)]
#[path = "state_machine_cell_test.rs"]
mod tests;
