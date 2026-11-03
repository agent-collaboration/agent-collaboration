//! Actual SDK engines and independent Inbox custody around Core's answer deadline.
//! Local session execution is an explicit Held consumer input; no positive ACK is forged.
use super::*;
use crate::{
    fleet_mail_tasks::{FleetMailRemoval, FleetPushTask, FleetStatusTask},
    fleet_sender_binding::FleetSenderBinding,
    fleet_storage_fixture::{FixtureIo, FleetStorageFixture, NOW, TestError},
};
use durable_steps::{
    AtomicStore, CallTarget, Engine, Host, NotSentReason, OperationId, Task,
    storage::{CallReach, JournalStorage, StepState, TransactionScope},
};
use fleet_sync_delivery::{
    DeliverCall, DeliverLocally, DeliveryConsumer, DeliveryLimits, DeliveryTaskConfig,
    GateDecision, Inbox, LocalDelivery, LocalDeliveryConfig, LocalOutcome, Outbox, PublishCall,
    ReportCall, ReportOutcome,
};
use fleet_sync_protocol::{
    DeliveryEnvelope, DeliveryHeader, DeliveryItem, PeerId, PushResult, Recipient, RecipientReport,
    ReportResult, SubmitResult, Submitter,
};
use fleet_sync_tokio_runner::TokioObject;
use runtime_primitives::{Clock, IdSource, SyncTime, WakeFailure, WakeScheduler};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::json;
use sqlx_core::{query::query, row::Row};
use std::{
    future::{pending, ready},
    sync::atomic::{AtomicI64, AtomicUsize, Ordering},
};

type Store = TokioObject<crate::CoreHost<FixtureIo>>;
type Push = FleetPushTask<Store, sqlx_turso::Turso>;
type Removal = FleetMailRemoval<Store, sqlx_turso::Turso>;
type Publication = FleetStatusTask<Store, sqlx_turso::Turso>;
type Attempt =
    DeliverLocally<Store, sqlx_turso::Turso, BoardDeliveryTypes, HeldSession, FleetSenderBinding>;
type Report = ReportOutcome<Store, sqlx_turso::Turso, BoardDeliveryTypes, FleetSenderBinding>;
type Sender = <BoardDeliveryTypes as DeliveryTypes>::Sender;
type TestResult<T = ()> = Result<T, TestError>;
const A: &str = "019f0000-0000-7000-8000-000000008a11";
const B: &str = "019f0000-0000-7000-8000-000000008a12";
const SERVICE_A: &str = "019f0000-0000-7000-8000-000000008b11";
const SERVICE_B: &str = "019f0000-0000-7000-8000-000000008b12";
const RELAY: &str = "019f0000-0000-7000-8000-000000005240";

struct HeldSession;
impl<S: TransactionScope> DeliveryConsumer<S> for HeldSession {
    type Protocol = BoardConsumerProtocol;
    async fn release_gate(
        &self,
        _: &mut S,
        _: &DeliveryItem<BoardDeliveryTypes>,
    ) -> Result<GateDecision, fleet_sync_delivery::DeliveryError> {
        Ok(GateDecision::Hold { until: None })
    }
    async fn deliver_locally(
        &self,
        _: &DeliveryItem<BoardDeliveryTypes>,
        _: CallId,
    ) -> LocalDelivery<BoardDeliveryTypes> {
        LocalDelivery {
            outcome: LocalOutcome::Held { until: None },
            observation: (),
        }
    }
    fn may_cancel(&self, _: &Sender, _: &DeliveryHeader<BoardDeliveryTypes>) -> bool {
        false
    }
}

#[derive(Clone, Copy)]
enum ReceiptPath {
    LoseAfterCommit,
    NoCapacity,
}

struct DeliveryHost {
    fleet: Arc<Store>,
    recipient: Arc<Store>,
    limits: DeliveryLimits,
    recipient_limits: DeliveryLimits,
    owner: PeerId,
    relay: PeerId,
    receipt: ReceiptPath,
    now: AtomicI64,
    commits: AtomicUsize,
    reports: AtomicUsize,
    report_ready: tokio::sync::Notify,
    report_release: tokio::sync::Notify,
}
impl Clock for DeliveryHost {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.now.load(Ordering::SeqCst))
    }
}
impl IdSource for DeliveryHost {
    fn new_id(&self) -> durable_steps::Uuid {
        durable_steps::Uuid::now_v7()
    }
}
impl WakeScheduler for DeliveryHost {
    async fn wake_at(&self, _: SyncTime) -> Result<(), WakeFailure> {
        Ok(())
    }
}
fn lost(_: impl std::fmt::Display) -> CallFailure {
    CallFailure::AnswerLost
}
fn cast<T: Serialize, U: DeserializeOwned>(value: &T) -> Result<U, CallFailure> {
    serde_json::from_slice(&serde_json::to_vec(value).map_err(lost)?).map_err(lost)
}
impl Host for DeliveryHost {
    async fn dispatch<C: CallTarget>(
        &self,
        target: C,
        request: C::Request,
        call: CallId,
    ) -> Result<C::Answer, CallFailure> {
        let now = self.now();
        let limits = self.limits;
        let owner = self.owner;
        match C::KIND {
            <fleet_sync_delivery::PushCall<BoardDeliveryTypes>>::KIND => {
                let reference: DeliveryItemRef = cast(&request)?;
                let resolved = self
                    .fleet
                    .transaction(move |scope| {
                        Box::pin(async move {
                            Outbox::<_, BoardDeliveryTypes, _>::new(
                                scope,
                                limits,
                                &FleetSenderBinding,
                            )
                            .resolve_push(&reference, now)
                            .await
                            .map_err(fleet_sync_delivery::delivery_rollback)
                        })
                    })
                    .await
                    .map_err(lost)?
                    .ok_or(CallFailure::AnswerLost)?;
                let recipient_limits = self.recipient_limits;
                let relay = self.relay;
                let receipt = self
                    .recipient
                    .transaction(move |scope| {
                        Box::pin(async move {
                            Inbox::<_, BoardDeliveryTypes, _>::new(
                                scope,
                                owner,
                                recipient_limits,
                                &FleetSenderBinding,
                            )
                            .accept_push::<Attempt, Report, Removal, _>(
                                resolved.submitter,
                                resolved.item,
                                relay,
                                &HeldSession,
                                now,
                            )
                            .await
                            .map_err(fleet_sync_delivery::delivery_rollback)
                        })
                    })
                    .await
                    .map_err(lost)?;
                let answer = match self.receipt {
                    ReceiptPath::LoseAfterCommit => {
                        if !matches!(receipt, PushResult::Accepted { .. }) {
                            return Err(CallFailure::AnswerLost);
                        }
                        // The actual Inbox transaction above has committed.
                        // Its real receipt is withheld; release only the injected
                        // answer deadline, never a manufactured positive ACK.
                        self.commits.fetch_add(1, Ordering::SeqCst);
                        bounded_answer(pending(), ready(())).await?
                    }
                    ReceiptPath::NoCapacity => {
                        bounded_answer(
                            ready(Ok(ClientAnswer::Crate(ServerCallAnswer::Pushed(receipt)))),
                            pending(),
                        )
                        .await?
                    }
                };
                cast(&answer)
            }
            <DeliverCall<BoardDeliveryTypes>>::KIND => {
                let reference: DeliveryItemRef = cast(&request)?;
                let resolved = self
                    .recipient
                    .transaction(move |scope| {
                        Box::pin(async move {
                            Inbox::<_, BoardDeliveryTypes, _>::new(
                                scope,
                                owner,
                                limits,
                                &FleetSenderBinding,
                            )
                            .resolve_local_delivery(&reference)
                            .await
                            .map_err(fleet_sync_delivery::delivery_rollback)
                        })
                    })
                    .await
                    .map_err(lost)?;
                let delivery = <HeldSession as DeliveryConsumer<
                    <Store as AtomicStore>::Scope<'_>,
                >>::deliver_locally(
                    &HeldSession, resolved.item(), call
                )
                .await;
                cast(&resolved.validated_answer(delivery))
            }
            <ReportCall<BoardDeliveryTypes>>::KIND => {
                let target: ReportCall<BoardDeliveryTypes> = cast(&target)?;
                if target.mailbox != self.owner || target.relay != self.relay {
                    return Err(CallFailure::AnswerLost);
                }
                let report: RecipientReport<BoardDeliveryTypes> = cast(&request)?;
                // Hold the real SDK-produced report after its saved local read,
                // before it reaches Fleet. No database scope remains open here.
                self.report_ready.notify_one();
                self.report_release.notified().await;
                let answer = self
                    .fleet
                    .transaction(move |scope| {
                        Box::pin(async move {
                            Outbox::<_, BoardDeliveryTypes, _>::new(
                                scope,
                                limits,
                                &FleetSenderBinding,
                            )
                            .recipient_report::<Publication>(owner, report, now)
                            .await
                            .map_err(fleet_sync_delivery::delivery_rollback)
                        })
                    })
                    .await
                    .map_err(lost)?;
                if !matches!(answer, ReportResult::Ack { .. }) {
                    return Err(CallFailure::AnswerLost);
                }
                self.reports.fetch_add(1, Ordering::SeqCst);
                cast(&answer)
            }
            <PublishCall<BoardDeliveryTypes>>::KIND => {
                // A's live status receiver is offline in this component case.
                // Real owed status remains pending; no Applied ACK is faked.
                Err(CallFailure::NotSent {
                    reason: NotSentReason::Unreachable { last_seen: None },
                })
            }
            _ => Err(CallFailure::AnswerLost),
        }
    }
}

struct Fixture {
    fleet: FleetStorageFixture,
    recipient: FleetStorageFixture,
    host: Arc<DeliveryHost>,
    fleet_engine: Engine<Store, DeliveryHost>,
    recipient_engine: Engine<Store, DeliveryHost>,
    reference: DeliveryItemRef,
    push: OperationId,
}
impl Fixture {
    async fn open(receipt: ReceiptPath) -> TestResult<Self> {
        let fleet = FleetStorageFixture::open().await?;
        let recipient = FleetStorageFixture::open().await?;
        for fixture in [&fleet, &recipient] {
            // Independent enrollment truth is supplied before any Push claim.
            fixture.enroll(A.to_owned().try_into()?, SERVICE_A).await?;
            fixture.enroll(B.to_owned().try_into()?, SERVICE_B).await?;
            fixture
                .object
                .transaction(|scope| {
                    Box::pin(async move {
                        fleet_sync_delivery::DELIVERY_MIGRATOR
                            .run_direct(None, scope.executor(), false)
                            .await
                            .map_err(StoreError::backend)
                    })
                })
                .await?;
        }
        let limits = crate::fleet_outbox_limits()?;
        let recipient_limits = match receipt {
            ReceiptPath::LoseAfterCommit => limits,
            ReceiptPath::NoCapacity => DeliveryLimits::new::<BoardDeliveryTypes>(
                limits.mailbox_payload_bytes(),
                limits.mailbox_items(),
                limits.outbox_bytes(),
                limits.outbox_items(),
                0,
                limits.settlement_allowance(),
            )?,
        };
        let owner = B.to_owned().try_into()?;
        let relay = RELAY.to_owned().try_into()?;
        let host = Arc::new(DeliveryHost {
            fleet: fleet.object.clone(),
            recipient: recipient.object.clone(),
            limits,
            recipient_limits,
            owner,
            relay,
            receipt,
            now: AtomicI64::new(NOW.unix_millis()),
            commits: AtomicUsize::new(0),
            reports: AtomicUsize::new(0),
            report_ready: tokio::sync::Notify::new(),
            report_release: tokio::sync::Notify::new(),
        });
        let binding = Arc::new(FleetSenderBinding);
        let mut fleet_engine = Engine::new(fleet.object.clone(), host.clone());
        fleet_engine.register(Push::new(limits, host.clone(), binding.clone()))?;
        fleet_engine.register(Publication::new())?;
        fleet_engine.register(Removal::new())?;
        let local = LocalDeliveryConfig { owner, limits };
        let mut recipient_engine = Engine::new(recipient.object.clone(), host.clone());
        recipient_engine.register(Attempt::new(
            local,
            Arc::new(HeldSession),
            host.clone(),
            binding.clone(),
        ))?;
        recipient_engine.register(Report::new(
            DeliveryTaskConfig {
                local,
                relay,
                fetch_items: 10,
                fetch_bytes: 1_000_000,
            },
            host.clone(),
            binding,
        ))?;
        recipient_engine.register(Removal::new())?;
        let item = DeliveryEnvelope::Item(DeliveryItem::new(
            fleet_sync_protocol::ItemId::new(durable_steps::Uuid::now_v7())?,
            serde_json::from_value(json!({"kind":"humanUser","installation":SERVICE_A}))?,
            Recipient {
                peer: owner,
                address: serde_json::from_value(
                    json!({"endpoint":{"serviceId":SERVICE_B,"endpointId":"codex-local"},"sessionId":"019f0000-0000-7000-8000-000000008c12"}),
                )?,
            },
            Some(NOW.after(std::time::Duration::from_millis(1_000))),
            serde_json::from_value(
                json!({"kind":"message","text":"committed before answer deadline","delivery":"queue","guard":null,"senderDisplayName":null,"replyTo":null}),
            )?,
        )?);
        let reference = DeliveryItemRef {
            item: item.id(),
            digest: item.digest().clone(),
        };
        let input = item;
        let push = fleet
            .object
            .transaction(move |scope| {
                Box::pin(async move {
                    let answer =
                        Outbox::<_, BoardDeliveryTypes, _>::new(scope, limits, &FleetSenderBinding)
                            .submit_relay::<Push, Removal>(
                                Submitter::Machine(
                                    A.to_owned().try_into().map_err(StoreError::backend)?,
                                ),
                                input,
                                relay,
                                SERVICE_A
                                    .to_owned()
                                    .try_into()
                                    .map_err(StoreError::backend)?,
                                NOW,
                            )
                            .await
                            .map_err(fleet_sync_delivery::delivery_rollback)?;
                    if !matches!(answer, SubmitResult::Accepted { .. }) {
                        return Err(StoreError::backend("real relay admission failed"));
                    }
                    let mut pushes = Vec::new();
                    for id in scope.due(NOW).await? {
                        if scope
                            .operation(id)
                            .await?
                            .ok_or(StoreError::MissingOperation { id })?
                            .kind
                            == Push::KIND
                        {
                            pushes.push(id);
                        }
                    }
                    let [push] = pushes.as_slice() else {
                        return Err(StoreError::backend("expected one real Push task"));
                    };
                    Ok(*push)
                })
            })
            .await?;
        Ok(Self {
            fleet,
            recipient,
            host,
            fleet_engine,
            recipient_engine,
            reference,
            push,
        })
    }
    async fn state(&self) -> TestResult<(String, i64, bool)> {
        let item = self.reference.item;
        Ok(self.fleet.object.transaction(move |scope|Box::pin(async move {
            let row=query::<sqlx_turso::Turso>("SELECT status_kind,CAST(status_version AS INTEGER) AS version,payload IS NOT NULL AS retained FROM _fleet_sync_outbox_items WHERE item_id=?")
                .bind(item.as_uuid().to_string()).fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
            Ok((row.try_get("status_kind").map_err(StoreError::backend)?,row.try_get("version").map_err(StoreError::backend)?,row.try_get::<i64,_>("retained").map_err(StoreError::backend)?==1))
        })).await?)
    }
    async fn reach(&self) -> TestResult<CallReach> {
        let id = self.push;
        Ok(self
            .fleet
            .object
            .transaction(move |scope| {
                Box::pin(async move {
                    let step = scope
                        .step(id, 0)
                        .await?
                        .ok_or_else(|| StoreError::backend("Push step absent"))?;
                    let StepState::CallPending { attempt, .. } = step.state else {
                        return Err(StoreError::backend("unanswered Push became terminal"));
                    };
                    Ok(attempt.reach)
                })
            })
            .await?)
    }
    async fn inbox_count(&self) -> TestResult<i64> {
        Ok(self
            .recipient
            .object
            .transaction(|scope| {
                Box::pin(async move {
                    query::<sqlx_turso::Turso>(
                        "SELECT count(*) AS count FROM _fleet_sync_inbox_items",
                    )
                    .fetch_one(scope.executor())
                    .await
                    .map_err(StoreError::backend)?
                    .try_get("count")
                    .map_err(StoreError::backend)
                })
            })
            .await?)
    }
    async fn close(self) -> TestResult {
        let Self {
            fleet,
            recipient,
            host,
            fleet_engine,
            recipient_engine,
            ..
        } = self;
        drop(fleet_engine);
        drop(recipient_engine);
        drop(host);
        recipient.close().await?;
        fleet.close().await
    }
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A real committed Inbox whose ACK timed out is possible reach: Fleet cannot invent Expired, and the actual SDK Held report must subsequently advance its retained status."
)]
async fn committed_inbox_answer_timeout_preserves_possible_reach_and_its_later_report_advances_sent()
-> TestResult {
    let fixture = Fixture::open(ReceiptPath::LoseAfterCommit).await?;
    let first = fixture.fleet_engine.drive_due().await?;
    assert!(first.failed.is_empty(), "{first:?}");
    assert!(first.retried.contains(&fixture.push));
    assert_eq!(fixture.host.commits.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.inbox_count().await?, 1);
    assert_eq!(fixture.state().await?, ("sent".into(), 1, true));
    let reach = fixture.reach().await?;
    // The SDK, not a handwritten report, records Held before TTL. Its report
    // task is deliberately not driven until after Fleet's expiry decision.
    let held = fixture.recipient_engine.drive_due().await?;
    assert!(held.failed.is_empty(), "{held:?}");
    assert_eq!(fixture.host.reports.load(Ordering::SeqCst), 0);
    let (reported, before_report) =
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            tokio::try_join!(fixture.recipient_engine.drive_due(), async {
                fixture.host.report_ready.notified().await;
                fixture
                    .host
                    .now
                    .store(NOW.unix_millis() + 1_001, Ordering::SeqCst);
                let observed = async {
                    let expired = fixture.fleet_engine.drive_due().await?;
                    if !expired.failed.is_empty() {
                        return Err(StoreError::backend(format!("{expired:?}")));
                    }
                    fixture.state().await.map_err(StoreError::backend)
                }
                .await;
                fixture.host.report_release.notify_one();
                observed
            })
        })
        .await??;
    assert_eq!(
        before_report,
        ("sent".into(), 1, false),
        "a lost real receipt was classified as definitely unsent; prior reach={reach:?}"
    );
    assert_eq!(reach, CallReach::MayHaveReached);
    assert!(reported.failed.is_empty(), "{reported:?}");
    assert_eq!(fixture.host.reports.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.state().await?, ("held".into(), 2, false));
    assert_eq!(fixture.inbox_count().await?, 1);
    fixture.close().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A real no-capacity Inbox refusal proves no intake and may truthfully expire at TTL, unlike a lost committed receipt."
)]
async fn actual_no_capacity_refusal_keeps_no_effect_and_expires_without_recipient_custody()
-> TestResult {
    let fixture = Fixture::open(ReceiptPath::NoCapacity).await?;
    let first = fixture.fleet_engine.drive_due().await?;
    assert!(first.failed.is_empty(), "{first:?}");
    assert!(first.retried.contains(&fixture.push));
    assert_eq!(fixture.reach().await?, CallReach::NoEffect);
    assert_eq!(fixture.inbox_count().await?, 0);
    fixture
        .host
        .now
        .store(NOW.unix_millis() + 1_001, Ordering::SeqCst);
    let expired = fixture.fleet_engine.drive_due().await?;
    assert!(expired.failed.is_empty(), "{expired:?}");
    assert_eq!(fixture.state().await?, ("expired".into(), 2, false));
    assert_eq!(fixture.inbox_count().await?, 0);
    assert_eq!(fixture.host.commits.load(Ordering::SeqCst), 0);
    fixture.close().await
}
