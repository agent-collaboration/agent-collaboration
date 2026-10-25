//! Real borrowed owner, Inbox, journal, runner and native actors; only carrier answers wait.
use super::native::{Runtime, accepted};
use crate::{MailPushDelivery, RouterConsumerProtocol, RouterDeliveryTypes};
use collaboration_mail_protocol::{CarrierAcceptance, MailSender};
use collaboration_protocol::{PushDeliveryState, PushId};
use durable_steps::{
    AtomicStore, CallFailure, CallId, CallTarget, Engine, NotSentReason, StoreError,
    storage::TransactionScope, turso_store::TursoScope,
};
use fleet_sync_delivery::{
    DeliverCall, DeliverLocally, DeliveryConsumer, DeliveryError, DeliveryTaskConfig, GateDecision,
    Inbox, LocalDelivery,
};
use fleet_sync_protocol::{
    DeliveryHeader, DeliveryItem, DeliveryItemRef, ItemId, RecipientOutcome,
};
use fleet_sync_tokio_runner::{CallEnvelope, CallRouter, TokioHost, TokioRunner};
use interaction_storage::{InteractionAtomicStore, mark_mail_push_attempted_in_scope};
use runtime_primitives::{Clock, SyncTime, WakeScheduler};
use serde_json::Value;
use std::{
    sync::{
        Arc, OnceLock, Weak,
        atomic::{AtomicI64, Ordering},
    },
    time::Duration,
};
use tokio::sync::watch;

type RunnerHost = TokioHost<RunnerRouter, RunnerClock>;
type RunnerStore = InteractionAtomicStore<RunnerHost>;
type RunnerDatabase = <TursoScope<'static, RunnerHost> as TransactionScope>::Database;
type RunnerTask = DeliverLocally<RunnerStore, RunnerDatabase, RouterDeliveryTypes, RunnerConsumer>;

const BOUNDARY_DEADLINE: Duration = Duration::from_secs(10);

struct RunnerClock(Arc<AtomicI64>);
impl Clock for RunnerClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.0.load(Ordering::SeqCst))
    }
}

struct RunnerConsumer {
    carrier: Arc<MailPushDelivery>,
    clock: Arc<AtomicI64>,
}
impl RunnerConsumer {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::from_timestamp_millis(self.clock.load(Ordering::SeqCst)).unwrap()
    }
}
impl DeliveryConsumer<TursoScope<'_, RunnerHost>> for RunnerConsumer {
    type Protocol = RouterConsumerProtocol;

    async fn on_received(
        &self,
        scope: &mut TursoScope<'_, RunnerHost>,
        item: &DeliveryItem<RouterDeliveryTypes>,
    ) -> Result<(), DeliveryError> {
        crate::project_received_mail(scope, item, self.now()).await
    }

    async fn on_outcome(
        &self,
        scope: &mut TursoScope<'_, RunnerHost>,
        item: ItemId,
        outcome: &RecipientOutcome<CarrierAcceptance>,
        observation: Option<&crate::RouterLocalObservation>,
    ) -> Result<(), DeliveryError> {
        crate::project_mail_outcome(scope, item, outcome, observation, self.now()).await
    }

    async fn release_gate(
        &self,
        _: &mut TursoScope<'_, RunnerHost>,
        _: &DeliveryItem<RouterDeliveryTypes>,
    ) -> Result<GateDecision, DeliveryError> {
        // Both items are local direct messages; no board release policy stands in for the runner.
        Ok(GateDecision::Release)
    }

    async fn deliver_locally(
        &self,
        item: &DeliveryItem<RouterDeliveryTypes>,
        call: CallId,
    ) -> LocalDelivery<RouterDeliveryTypes> {
        self.carrier
            .deliver_push(item, &call)
            .await
            .expect("real native actor answers")
            .expect("direct messages prepare a native push")
    }

    fn may_cancel(&self, sender: &MailSender, item: &DeliveryHeader<RouterDeliveryTypes>) -> bool {
        sender == item.sender()
    }
}

struct RunnerRouter {
    store: OnceLock<Weak<RunnerStore>>,
    consumer: Arc<RunnerConsumer>,
    config: DeliveryTaskConfig,
}
impl CallRouter for RunnerRouter {
    async fn dispatch(&self, envelope: CallEnvelope) -> Result<Value, CallFailure> {
        if envelope.kind != DeliverCall::<RouterDeliveryTypes>::KIND {
            return Err(CallFailure::NotSent {
                reason: NotSentReason::Overloaded,
            });
        }
        let _: DeliverCall<RouterDeliveryTypes> =
            serde_json::from_value(envelope.target).map_err(|_| CallFailure::AnswerLost)?;
        let reference: DeliveryItemRef =
            serde_json::from_value(envelope.request).map_err(|_| CallFailure::AnswerLost)?;
        let store = self
            .store
            .get()
            .and_then(Weak::upgrade)
            .ok_or(CallFailure::AnswerLost)?;
        let config = self.config;
        let resolved = store
            .transaction(move |scope| {
                Box::pin(async move {
                    let resolved = Inbox::<_, RouterDeliveryTypes>::new(
                        scope,
                        config.local.owner,
                        config.local.limits,
                    )
                    .resolve_local_delivery(&reference)
                    .await
                    .map_err(StoreError::backend)?;
                    let item = resolved.item();
                    let push_id: PushId = item
                        .id()
                        .as_uuid()
                        .to_string()
                        .try_into()
                        .map_err(StoreError::backend)?;
                    mark_mail_push_attempted_in_scope(scope, &push_id)
                        .await
                        .map_err(StoreError::backend)?;
                    Ok(resolved)
                })
            })
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        let delivery = self
            .consumer
            .deliver_locally(resolved.item(), envelope.call)
            .await;
        serde_json::to_value(resolved.validated_answer(delivery))
            .map_err(|_| CallFailure::AnswerLost)
    }
}

#[tokio::test]
async fn a_blocked_native_reader_does_not_block_another_due_mail_item() {
    // Arrange: one physical owner and actual per-reader native actors. The existing
    // fixture's relay is a named stand-in, unused by these local items.
    let runtime = Runtime::new().await;
    let first = runtime.fixture.draft();
    let first_push = first.push_id.clone();
    let first_item = runtime.fixture.item(&first).id();
    let mut second = runtime.fixture.draft();
    second.target.session_id = "independent-second-reader".to_owned().try_into().unwrap();
    assert_ne!(first.target, second.target);
    let second_push = second.push_id.clone();
    let second_item = runtime.fixture.item(&second).id();
    runtime.client.script(first_item, vec![accepted()]);
    runtime.client.script(second_item, vec![accepted()]);
    let first_pause = runtime.client.pause_before_answer(first_item);
    let mut second_pause = runtime.client.pause_before_answer(second_item);
    runtime.fixture.admit_local(first).await.unwrap();

    let consumer = Arc::new(RunnerConsumer {
        carrier: runtime
            .fixture
            .host
            .consumer
            .native
            .lock()
            .unwrap()
            .clone()
            .unwrap(),
        clock: runtime.fixture.host.clock.clone(),
    });
    let router = Arc::new(RunnerRouter {
        store: OnceLock::new(),
        consumer: consumer.clone(),
        config: runtime.fixture.host.config,
    });
    let host = Arc::new(TokioHost::new(
        router.clone(),
        RunnerClock(runtime.fixture.host.clock.clone()),
    ));
    let store = Arc::new(InteractionAtomicStore::new(
        runtime.fixture.owner.clone(),
        host.clone(),
    ));
    assert!(router.store.set(Arc::downgrade(&store)).is_ok());
    let mut engine = Engine::new(store.clone(), host.clone());
    engine
        .register(RunnerTask::new(
            router.config.local,
            consumer.clone(),
            host.clone(),
        ))
        .unwrap();
    let runner = TokioRunner::new(Arc::new(engine), host.clone());
    let (stop, stopped) = watch::channel(false);
    let mut running = tokio::spawn(async move { runner.run(stopped).await });
    tokio::time::timeout(BOUNDARY_DEADLINE, first_pause.entered)
        .await
        .unwrap()
        .unwrap();

    // Act: commit independent work while the first reader's real native call is held,
    // and signal the public host wake only after that admission commits.
    runtime.fixture.admit_local(second).await.unwrap();
    host.wake_at(host.now()).await.unwrap();
    let second_before_release =
        tokio::time::timeout(Duration::from_secs(5), &mut second_pause.entered).await;
    let independent_progress = matches!(second_before_release, Ok(Ok(())));

    // Drain even on the expected pre-fix red. This ensures the eventual assertions
    // distinguish a runner stall from missing work, a failed carrier or a leaked task.
    first_pause.release.send(()).unwrap();
    if second_before_release.is_err() {
        tokio::time::timeout(BOUNDARY_DEADLINE, &mut second_pause.entered)
            .await
            .unwrap()
            .unwrap();
    }
    second_pause.release.send(()).unwrap();
    stop.send_replace(true);
    let drained = tokio::time::timeout(BOUNDARY_DEADLINE, &mut running).await;
    if drained.is_err() {
        running.abort();
        let _ = running.await;
        panic!("the runner did not drain after both native calls were released");
    }
    drained.unwrap().unwrap().unwrap();

    // Assert: both real outcomes commit through the same owner, once per item.
    assert_eq!(runtime.client.calls(first_item), 1);
    assert_eq!(runtime.client.calls(second_item), 1);
    for push_id in [first_push, second_push] {
        assert_eq!(
            runtime
                .fixture
                .owner
                .get_push_record(&push_id)
                .await
                .unwrap()
                .unwrap()
                .delivery_state,
            PushDeliveryState::Delivered
        );
    }
    drop(store);
    drop(host);
    drop(router);
    drop(consumer);
    runtime.close().await;
    assert!(
        independent_progress,
        "SDK_RUNNER_READER_STALLED: the second due reader reached native delivery only after the first was released"
    );
}
