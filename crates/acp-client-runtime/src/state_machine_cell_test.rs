#![deny(clippy::wildcard_enum_match_arm)]
use super::*;
use state_machine_core::{EventTag, RefusalTag, StateMachineResult, StateTag, Transition};
use std::sync::atomic::AtomicUsize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Count(u32);

impl StateTag for Count {
    fn state_tag(&self) -> &'static str {
        "count"
    }
    fn is_terminal(&self) -> bool {
        false
    }
}

#[derive(Clone, Copy, Debug)]
enum Step {
    Add(u32),
    /// Its effect applies `Add` back to the same cell.
    Echo(u32),
    Refuse,
}

impl EventTag for Step {
    fn event_tag(&self) -> &'static str {
        match self {
            Self::Add(_) => "add",
            Self::Echo(_) => "echo",
            Self::Refuse => "refuse",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Effect {
    Record(u32),
    EchoBack(u32),
}

#[derive(Debug, Eq, PartialEq)]
enum Refusal {
    Always,
}

impl RefusalTag for Refusal {
    fn refusal_tag(&self) -> &'static str {
        match self {
            Self::Always => "always",
        }
    }
}

struct Counter;

impl StateMachine for Counter {
    type State = Count;
    type Event = Step;
    type Effect = Effect;
    type Error = Refusal;

    fn transition(state: &Count, event: Step) -> StateMachineResult<Self> {
        match event {
            Step::Add(n) => Ok((Count(state.0 + n), vec![Effect::Record(n)])),
            Step::Echo(n) => Ok((*state, vec![Effect::EchoBack(n)])),
            Step::Refuse => Err(TransitionError::Domain(Refusal::Always)),
        }
    }

    fn transitions() -> &'static [Transition] {
        &[]
    }
}

#[derive(Default)]
struct Recorder {
    applied: Mutex<Vec<u32>>,
    running: AtomicUsize,
    overlapped: AtomicBool,
}

impl CellEffects<Counter> for Recorder {
    fn apply_effect<'a>(
        &'a self,
        cell: &'a Arc<MachineCell<Counter>>,
        effect: Effect,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if self.running.fetch_add(1, Ordering::SeqCst) > 0 {
                self.overlapped.store(true, Ordering::SeqCst);
            }
            tokio::task::yield_now().await;
            match effect {
                Effect::Record(n) => self
                    .applied
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(n),
                Effect::EchoBack(n) => {
                    let _applied = cell.apply(Step::Add(n));
                }
            }
            self.running.fetch_sub(1, Ordering::SeqCst);
        })
    }
}

fn cell(recorder: &Arc<Recorder>, runtime: &Arc<CellRuntime>) -> Arc<MachineCell<Counter>> {
    MachineCell::new(
        Count(0),
        Arc::clone(recorder) as Arc<dyn CellEffects<Counter>>,
        Arc::clone(runtime),
    )
}

/// Oracle: design note §11.1. Effects run in the order their transitions
/// committed, one runner per cell at a time, and an effect can apply an event
/// to its own cell without deadlock.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn effects_run_in_commit_order_one_runner_at_a_time() {
    // Arrange
    let recorder = Arc::new(Recorder::default());
    let runtime = Arc::new(CellRuntime::default());
    let cell = cell(&recorder, &runtime);

    // Act
    for n in 1..=50 {
        let event = if n % 10 == 0 {
            Step::Echo(n)
        } else {
            Step::Add(n)
        };
        cell.apply(event).expect("applied");
    }
    runtime.drain().await;

    // Assert
    let applied = recorder
        .applied
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let expected_sum: u32 = (1..=50).sum();
    assert_eq!(cell.current(), Count(expected_sum));
    let direct: Vec<u32> = applied.iter().copied().filter(|n| n % 10 != 0).collect();
    assert_eq!(
        direct,
        (1..=50).filter(|n| n % 10 != 0).collect::<Vec<u32>>()
    );
    assert_eq!(
        applied.len(),
        50,
        "every add, direct or echoed, was recorded"
    );
    assert!(
        !recorder.overlapped.load(Ordering::SeqCst),
        "two runners overlapped"
    );
    assert_eq!(cell.pending_effects(), 0);
}

/// Oracle: M2 and M6. A refusal changes nothing and appends no effect.
#[tokio::test]
async fn a_refusal_changes_nothing() {
    // Arrange
    let recorder = Arc::new(Recorder::default());
    let runtime = Arc::new(CellRuntime::default());
    let cell = cell(&recorder, &runtime);

    // Act
    let refused = cell.apply(Step::Refuse);
    runtime.drain().await;

    // Assert
    assert!(matches!(
        refused,
        Err(TransitionError::Domain(Refusal::Always))
    ));
    assert_eq!(cell.current(), Count(0));
    assert!(
        recorder
            .applied
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty()
    );
}

/// Oracle: §13.3's barrier. After the drain, a transition still commits but
/// no runner starts; its effects wait unapplied.
#[tokio::test]
async fn no_runner_starts_after_the_drain() {
    // Arrange
    let recorder = Arc::new(Recorder::default());
    let runtime = Arc::new(CellRuntime::default());
    let cell = cell(&recorder, &runtime);
    runtime.drain().await;

    // Act
    let applied = cell.apply(Step::Add(3));
    let spawned = runtime.spawn_tracked(async {});

    // Assert
    assert_eq!(applied, Ok(Count(3)));
    assert_eq!(cell.pending_effects(), 1);
    assert!(!spawned);
}
