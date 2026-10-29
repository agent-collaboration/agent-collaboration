//! One captured UTC clock and existing completed-owner facts for the real Host.
use chrono::{DateTime, Utc};
use codex_router_host::CollaborationRuntime;
use codex_router_test_support::FixtureResult;
use collaboration_service::SubscriptionClock;
use std::{future::Future, sync::Mutex, time::Duration};
use tokio::sync::watch;
use tracing::field::{Field, Visit};
use tracing_subscriber::{Layer, layer::Context, prelude::*};

pub(crate) struct ControlledHostWakeClock(Mutex<DateTime<Utc>>);
impl ControlledHostWakeClock {
    pub(crate) fn new() -> Self {
        Self(Mutex::new(Utc::now()))
    }

    pub(crate) async fn advance(&self, duration: Duration) -> FixtureResult {
        {
            let mut now = self
                .0
                .lock()
                .map_err(|_| "controlled wake clock poisoned")?;
            *now = now
                .checked_add_signed(chrono::Duration::from_std(duration)?)
                .ok_or("controlled wake clock advance out of range")?;
        }
        tokio::time::pause();
        tokio::time::advance(duration).await;
        tokio::time::resume();
        Ok(())
    }
}
impl SubscriptionClock for ControlledHostWakeClock {
    fn now(&self) -> DateTime<Utc> {
        match self.0.lock() {
            Ok(now) => *now,
            Err(poisoned) => *poisoned.into_inner(),
        }
    }
    fn monotonic_now(&self) -> tokio::time::Instant {
        tokio::time::Instant::now()
    }
}

pub(crate) async fn drive_wake_until<Waiting: Future>(
    runtime: &CollaborationRuntime,
    clock: &ControlledHostWakeClock,
    waiting: Waiting,
) -> FixtureResult<Waiting::Output> {
    let mut passes = runtime.runtime_test_observe_wake_progress()?;
    tokio::pin!(waiting);
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let progressing = async {
                let active_dispatches = *passes.borrow_and_update();
                if active_dispatches == 0 {
                    clock.advance(Duration::from_millis(250)).await?;
                }
                passes.changed().await?;
                FixtureResult::Ok(())
            };
            tokio::select! {
                biased;
                result = &mut waiting => return FixtureResult::Ok(result),
                progress = progressing => progress?,
            }
        }
    })
    .await?
}

struct ItemField<'expected> {
    expected: &'expected str,
    matched: bool,
}
impl Visit for ItemField<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "item_id" && format!("{value:?}") == self.expected {
            self.matched = true;
        }
    }
}
struct CommittedWakeLayer {
    expected: String,
    committed: watch::Sender<bool>,
}
impl<Subscriber: tracing::Subscriber> Layer<Subscriber> for CommittedWakeLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _context: Context<'_, Subscriber>) {
        if event.metadata().target() != "collaboration_service::runtime_test_remote_wake_commit" {
            return;
        }
        let mut item = ItemField {
            expected: &self.expected,
            matched: false,
        };
        event.record(&mut item);
        if item.matched {
            self.committed.send_replace(true);
        }
    }
}

pub(crate) fn observe_committed_wake(
    operation: &collaboration_protocol::OperationId,
) -> (impl tracing::Subscriber, watch::Receiver<bool>) {
    let (committed, observed) = watch::channel(false);
    (
        tracing_subscriber::registry().with(CommittedWakeLayer {
            expected: operation.as_str().to_owned(),
            committed,
        }),
        observed,
    )
}

pub(crate) async fn expire_after_commit<Waiting: Future>(
    clock: &ControlledHostWakeClock,
    mut committed: watch::Receiver<bool>,
    waiting: Waiting,
) -> FixtureResult<Waiting::Output>
where
    Waiting::Output: std::fmt::Debug,
{
    tokio::pin!(waiting);
    tokio::time::timeout(Duration::from_secs(20), async {
        tokio::select! {
            result = &mut waiting => FixtureResult::Err(format!("wake settled before its actual outbox commit: {result:?}").into()),
            observed = committed.wait_for(|committed| *committed) => observed.map(|_| ()).map_err(Into::into),
        }
    }).await??;
    clock.advance(Duration::from_millis(1001)).await?;
    tokio::time::timeout(Duration::from_secs(20), waiting)
        .await
        .map_err(Into::into)
}

struct DeadlineField(Option<Duration>);
impl Visit for DeadlineField {
    fn record_u64(&mut self, field: &Field, value: u64) {
        if field.name() == "timeout_ms" {
            self.0 = Some(Duration::from_millis(value));
        }
    }
    fn record_debug(&mut self, _field: &Field, _value: &dyn std::fmt::Debug) {}
}
struct CallerDeadlineLayer(watch::Sender<Vec<Duration>>);
impl<Subscriber: tracing::Subscriber> Layer<Subscriber> for CallerDeadlineLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _context: Context<'_, Subscriber>) {
        if event.metadata().target() != "collaboration_service::runtime_test_remote_wake_deadline" {
            return;
        }
        let mut deadline = DeadlineField(None);
        event.record(&mut deadline);
        if let Some(duration) = deadline.0 {
            self.0.send_modify(|deadlines| deadlines.push(duration));
        }
    }
}

pub(crate) fn observe_caller_deadlines()
-> (impl tracing::Subscriber, watch::Receiver<Vec<Duration>>) {
    let (armed, observed) = watch::channel(Vec::new());
    (
        tracing_subscriber::registry().with(CallerDeadlineLayer(armed)),
        observed,
    )
}

pub(crate) async fn expire_management_after_arming<Waiting: Future>(
    clock: &ControlledHostWakeClock,
    mut deadlines: watch::Receiver<Vec<Duration>>,
    waiting: Waiting,
) -> FixtureResult<Waiting::Output> {
    tokio::pin!(waiting);
    let already_completed = tokio::time::timeout(Duration::from_secs(25), async {
        tokio::select! {
            result = &mut waiting => FixtureResult::Ok(Some(result)),
            armed = deadlines.wait_for(|armed| armed.len() == 4) => {
                armed?;
                FixtureResult::Ok(None)
            }
        }
    })
    .await??;
    if let Some(result) = already_completed {
        return Ok(result);
    }
    let duration = deadlines
        .borrow()
        .iter()
        .copied()
        .max()
        .ok_or("no actual management deadline")?;
    clock.advance(duration + Duration::from_millis(1)).await?;
    tokio::time::timeout(Duration::from_secs(25), waiting)
        .await
        .map_err(Into::into)
}
