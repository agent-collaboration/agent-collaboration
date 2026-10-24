//! Real Inbox, journal and owning store; only the external carrier answer stands in.
use super::super::*;
use collaboration_protocol::{PushHeaderFacts, PushId, PushKind, PushOrigin, PushRecordDraft};
use durable_steps::{
    AtomicStore, CallFailure, CallId, CallTarget, Engine, Host, JournalScope, NotSentReason,
    StoreError, storage::TransactionScope, turso_store::TursoScope,
};
use fleet_sync_delivery::{
    DeliverCall, DeliverLocally, DeliveryConsumer, DeliveryLimits, DeliveryTaskConfig,
    GateDecision, Inbox, LocalDelivery, LocalDeliveryConfig, LocalItemAdmission, LocalOutcome,
    minimum_settlement_allowance,
};
use fleet_sync_protocol::{
    DeliveryEnvelope, DeliveryHeader, DeliveryItemRef, DeliverySeq, DeliveryStream, FetchEntry,
    FetchResult, PeerId, SyncTime,
};
use interaction_storage::{
    InteractionAtomicStore, InteractionStore, mark_mail_push_attempted_in_scope,
    record_message_send_admission,
};
use runtime_primitives::{Clock, IdSource, WakeFailure, WakeScheduler};
use serde_json::json;
use sqlx::Row;
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex, OnceLock, Weak,
        atomic::{AtomicI64, AtomicUsize, Ordering},
    },
};

pub type ProjectionStore = InteractionAtomicStore<ProjectionHost>;
type ProjectionDatabase = <TursoScope<'static, ProjectionHost> as TransactionScope>::Database;
pub type ProjectionTask =
    DeliverLocally<ProjectionStore, ProjectionDatabase, RouterDeliveryTypes, ProjectionConsumer>;

pub struct ProjectionConsumer {
    pub answers: Mutex<VecDeque<LocalDelivery<RouterDeliveryTypes>>>,
    pub native: Mutex<Option<Arc<crate::MailPushDelivery>>>,
    clock: Arc<AtomicI64>,
}
impl ProjectionConsumer {
    fn now(&self) -> DateTime<Utc> {
        DateTime::from_timestamp_millis(self.clock.load(Ordering::SeqCst)).unwrap()
    }
}
impl DeliveryConsumer<TursoScope<'_, ProjectionHost>> for ProjectionConsumer {
    type Protocol = crate::RouterConsumerProtocol;
    async fn on_received(
        &self,
        scope: &mut TursoScope<'_, ProjectionHost>,
        item: &DeliveryItem<RouterDeliveryTypes>,
    ) -> Result<(), DeliveryError> {
        project_received_mail(scope, item, self.now()).await
    }
    async fn on_outcome(
        &self,
        scope: &mut TursoScope<'_, ProjectionHost>,
        item: ItemId,
        outcome: &RecipientOutcome<CarrierAcceptance>,
        observation: Option<&crate::RouterLocalObservation>,
    ) -> Result<(), DeliveryError> {
        project_mail_outcome(scope, item, outcome, observation, self.now()).await
    }
    async fn on_cancel_outcome(
        &self,
        scope: &mut TursoScope<'_, ProjectionHost>,
        cancel: ItemId,
        target: ItemId,
        outcome: &RecipientOutcome<CarrierAcceptance>,
    ) -> Result<(), DeliveryError> {
        if let RecipientOutcome::TooLate { at, .. } = outcome {
            project_cancelled_after_delivery(scope, cancel, target, *at, self.now()).await?;
        }
        Ok(())
    }
    async fn release_gate(
        &self,
        _: &mut TursoScope<'_, ProjectionHost>,
        _: &DeliveryItem<RouterDeliveryTypes>,
    ) -> Result<GateDecision, DeliveryError> {
        Ok(GateDecision::Release)
    }
    async fn deliver_locally(
        &self,
        item: &DeliveryItem<RouterDeliveryTypes>,
        call: CallId,
    ) -> LocalDelivery<RouterDeliveryTypes> {
        let native = self.native.lock().unwrap().clone();
        if let Some(native) = native {
            return native.deliver_push(item, &call).await.unwrap().unwrap();
        }
        self.answers
            .lock()
            .unwrap()
            .pop_front()
            .expect("one explicit external answer per actual call")
    }
    fn may_cancel(
        &self,
        sender: &collaboration_mail_protocol::MailSender,
        header: &DeliveryHeader<RouterDeliveryTypes>,
    ) -> bool {
        sender == header.sender()
    }
}

pub struct ProjectionHost {
    pub clock: Arc<AtomicI64>,
    pub calls: AtomicUsize,
    pub consumer: Arc<ProjectionConsumer>,
    pub store: OnceLock<Weak<ProjectionStore>>,
    pub config: DeliveryTaskConfig,
    dispatch_checkpoint: Mutex<Option<DispatchCheckpoint>>,
}
struct DispatchCheckpoint {
    entered: tokio::sync::oneshot::Sender<()>,
    resume: tokio::sync::oneshot::Receiver<()>,
}
pub struct DispatchWitness {
    pub entered: tokio::sync::oneshot::Receiver<()>,
    pub resume: tokio::sync::oneshot::Sender<()>,
}
impl Clock for ProjectionHost {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.clock.load(Ordering::SeqCst))
    }
}
impl IdSource for ProjectionHost {
    fn new_id(&self) -> durable_steps::Uuid {
        uuid::Uuid::now_v7()
    }
}
impl WakeScheduler for ProjectionHost {
    async fn wake_at(&self, _: SyncTime) -> Result<(), WakeFailure> {
        Ok(())
    }
}
impl Host for ProjectionHost {
    async fn dispatch<T: CallTarget>(
        &self,
        _: T,
        request: T::Request,
        call: CallId,
    ) -> Result<T::Answer, CallFailure> {
        if T::KIND != DeliverCall::<RouterDeliveryTypes>::KIND {
            return Err(CallFailure::NotSent {
                reason: NotSentReason::Overloaded,
            });
        }
        let request = serde_json::to_value(request).map_err(|_| CallFailure::AnswerLost)?;
        let reference: DeliveryItemRef =
            serde_json::from_value(request).map_err(|_| CallFailure::AnswerLost)?;
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
                    let push_id = item
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
        self.calls.fetch_add(1, Ordering::SeqCst);
        let answer =
            resolved.validated_answer(self.consumer.deliver_locally(resolved.item(), call).await);
        let checkpoint = self.dispatch_checkpoint.lock().unwrap().take();
        if let Some(checkpoint) = checkpoint {
            checkpoint
                .entered
                .send(())
                .map_err(|_| CallFailure::AnswerLost)?;
            checkpoint
                .resume
                .await
                .map_err(|_| CallFailure::AnswerLost)?;
        }
        serde_json::from_value(serde_json::to_value(answer).map_err(|_| CallFailure::AnswerLost)?)
            .map_err(|_| CallFailure::AnswerLost)
    }
}

pub struct Fixture {
    pub owner: Arc<InteractionStore>,
    pub store: Arc<ProjectionStore>,
    pub host: Arc<ProjectionHost>,
    directory: tempfile::TempDir,
}
pub fn recorded_at() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-19T18:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}
impl Fixture {
    pub fn database_path(&self) -> std::path::PathBuf {
        self.directory.path().join("interaction.sqlite")
    }

    pub async fn new() -> Self {
        let owner = PeerId::new(uuid::Uuid::now_v7()).unwrap();
        Self::open(tempfile::tempdir().unwrap(), owner).await
    }
    async fn open(directory: tempfile::TempDir, peer: PeerId) -> Self {
        let owner = Arc::new(
            InteractionStore::open(directory.path().join("interaction.sqlite"))
                .await
                .unwrap(),
        );
        let limits = DeliveryLimits::new::<RouterDeliveryTypes>(
            16 * 1024 * 1024,
            30,
            16 * 1024 * 1024,
            30,
            16 * 1024 * 1024,
            minimum_settlement_allowance::<RouterDeliveryTypes>().unwrap(),
        )
        .unwrap();
        let clock = Arc::new(AtomicI64::new(recorded_at().timestamp_millis()));
        let host = Arc::new(ProjectionHost {
            clock: clock.clone(),
            calls: AtomicUsize::new(0),
            consumer: Arc::new(ProjectionConsumer {
                answers: Mutex::new(VecDeque::new()),
                native: Mutex::new(None),
                clock,
            }),
            store: OnceLock::new(),
            dispatch_checkpoint: Mutex::new(None),
            config: DeliveryTaskConfig {
                local: LocalDeliveryConfig {
                    owner: peer,
                    limits,
                },
                relay: PeerId::new(uuid::Uuid::now_v7()).unwrap(),
                fetch_items: 10,
                fetch_bytes: 1024 * 1024,
            },
        });
        let store = Arc::new(InteractionAtomicStore::new(owner.clone(), host.clone()));
        assert!(host.store.set(Arc::downgrade(&store)).is_ok());
        Self {
            owner,
            store,
            host,
            directory,
        }
    }
    pub fn engine(&self) -> Engine<ProjectionStore, ProjectionHost> {
        let mut engine = Engine::new(self.store.clone(), self.host.clone());
        engine
            .register(ProjectionTask::new(
                self.host.config.local,
                self.host.consumer.clone(),
                self.host.clone(),
            ))
            .unwrap();
        engine
    }
    pub fn pause_after_external_answer(&self) -> DispatchWitness {
        let (entered, observed) = tokio::sync::oneshot::channel();
        let (resume, waiting) = tokio::sync::oneshot::channel();
        assert!(
            self.host
                .dispatch_checkpoint
                .lock()
                .unwrap()
                .replace(DispatchCheckpoint {
                    entered,
                    resume: waiting,
                })
                .is_none()
        );
        DispatchWitness {
            entered: observed,
            resume,
        }
    }
    pub fn draft(&self) -> PushRecordDraft {
        PushRecordDraft {
            push_id:PushId::try_from(uuid::Uuid::now_v7().to_string()).unwrap(),kind:PushKind::DirectMessage,origin:PushOrigin::OwnerUnverified,origin_router_ref:None,
            target:serde_json::from_value(json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"projection-recipient"})).unwrap(),
            mode:Some(collaboration_protocol::MessageDelivery::Auto),guard:None,reply_to_push_id:None,
            header_facts:PushHeaderFacts::DirectMessage {sender_display_name:None},body:Some("One durable message".to_owned()),activity:None,created_at:recorded_at(),
        }
    }
    pub fn item(&self, draft: &PushRecordDraft) -> DeliveryItem<RouterDeliveryTypes> {
        crate::outgoing_mail_message(
            draft,
            &"00000000-0000-4000-8000-000000000001"
                .to_owned()
                .try_into()
                .unwrap(),
            self.host.config.local.owner,
        )
        .unwrap()
    }
    pub async fn admit_local(&self, draft: PushRecordDraft) -> Result<bool, StoreError> {
        let item = self.item(&draft);
        let config = self.host.config;
        let consumer = self.host.consumer.clone();
        self.store
            .transaction(move |scope| {
                Box::pin(async move {
                    let item_id = item
                        .id()
                        .as_uuid()
                        .to_string()
                        .try_into()
                        .map_err(StoreError::backend)?;
                    let admitted = Inbox::<_, RouterDeliveryTypes>::new(
                        scope,
                        config.local.owner,
                        config.local.limits,
                    )
                    .enqueue_local_and_admit::<ProjectionTask, _>(
                        item,
                        consumer.as_ref(),
                        SyncTime::from_unix_millis(recorded_at().timestamp_millis()),
                    )
                    .await
                    .map_err(StoreError::backend)?;
                    let attempted = matches!(admitted, LocalItemAdmission::Attempt(_));
                    if attempted {
                        record_message_send_admission(
                            scope,
                            &item_id,
                            "🤖 frozen recipient",
                            recorded_at(),
                        )
                        .await
                        .map_err(StoreError::backend)?;
                        // Local admission does not call the remote receive hook; the caller projects here.
                        insert_mail_push_in_scope(scope, draft)
                            .await
                            .map_err(StoreError::backend)?;
                    }
                    Ok(attempted)
                })
            })
            .await
    }
    pub async fn receive(&self, draft: PushRecordDraft) -> Result<(), StoreError> {
        let item = self.item(&draft);
        self.receive_items(vec![item]).await
    }
    pub async fn receive_items(
        &self,
        items: Vec<DeliveryItem<RouterDeliveryTypes>>,
    ) -> Result<(), StoreError> {
        let config = self.host.config;
        let consumer = self.host.consumer.clone();
        self.store
            .transaction(move |scope| {
                Box::pin(async move {
                    let mut inbox = Inbox::<_, RouterDeliveryTypes>::new(
                        scope,
                        config.local.owner,
                        config.local.limits,
                    );
                    let input = inbox
                        .begin_receive(DeliveryStream::Items)
                        .await
                        .map_err(StoreError::backend)?
                        .ok_or_else(|| StoreError::backend("receive fixture cycle absent"))?;
                    let effects = inbox
                        .store_batch(
                            input,
                            FetchResult {
                                stream: DeliveryStream::Items,
                                through: DeliverySeq::new(
                                    u64::try_from(items.len()).map_err(StoreError::backend)?,
                                ),
                                entries: items
                                    .into_iter()
                                    .enumerate()
                                    .map(|(index, item)| {
                                        Ok(FetchEntry::Item {
                                            seq: DeliverySeq::new(
                                                u64::try_from(index)
                                                    .map_err(StoreError::backend)?
                                                    + 1,
                                            ),
                                            item: DeliveryEnvelope::Item(item),
                                        })
                                    })
                                    .collect::<Result<Vec<_>, StoreError>>()?,
                            },
                            consumer.as_ref(),
                            SyncTime::from_unix_millis(recorded_at().timestamp_millis()),
                        )
                        .await
                        .map_err(StoreError::backend)?;
                    for attempt in effects.attempts {
                        let key = fleet_sync_delivery::delivery_request_key(&(
                            attempt.item,
                            attempt.attempt,
                        ))
                        .map_err(StoreError::backend)?;
                        scope
                            .admit::<ProjectionTask>(&key, &attempt)
                            .await
                            .map_err(StoreError::backend)?;
                    }
                    Ok(())
                })
            })
            .await
    }
    pub async fn receive_cancel(&self, draft: &PushRecordDraft) -> ItemId {
        let original = self.item(draft);
        let control = ItemId::new(uuid::Uuid::now_v7()).unwrap();
        let cancel = fleet_sync_protocol::DeliveryCancel::<RouterDeliveryTypes>::new(
            control,
            original.sender().clone(),
            original.recipient().clone(),
            None,
            fleet_sync_protocol::CancelBody {
                target: original.id(),
            },
        )
        .unwrap();
        let config = self.host.config;
        let consumer = self.host.consumer.clone();
        self.store
            .transaction(move |scope| {
                Box::pin(async move {
                    let mut inbox = Inbox::<_, RouterDeliveryTypes>::new(
                        scope,
                        config.local.owner,
                        config.local.limits,
                    );
                    let input = inbox
                        .begin_receive(DeliveryStream::Controls)
                        .await
                        .map_err(StoreError::backend)?
                        .ok_or_else(|| StoreError::backend("control fixture cycle absent"))?;
                    inbox
                        .store_batch(
                            input,
                            FetchResult {
                                stream: DeliveryStream::Controls,
                                through: DeliverySeq::new(1),
                                entries: vec![FetchEntry::Item {
                                    seq: DeliverySeq::new(1),
                                    item: DeliveryEnvelope::Cancel(cancel),
                                }],
                            },
                            consumer.as_ref(),
                            SyncTime::from_unix_millis(recorded_at().timestamp_millis()),
                        )
                        .await
                        .map_err(StoreError::backend)?;
                    Ok(())
                })
            })
            .await
            .unwrap();
        control
    }
    pub async fn report(&self, item: ItemId) -> Option<RecipientOutcome<CarrierAcceptance>> {
        let config = self.host.config;
        self.store
            .transaction(move |scope| {
                Box::pin(async move {
                    let mut inbox = Inbox::<_, RouterDeliveryTypes>::new(
                        scope,
                        config.local.owner,
                        config.local.limits,
                    );
                    let Some(reference) = inbox
                        .pending_report(item)
                        .await
                        .map_err(StoreError::backend)?
                    else {
                        return Ok(None);
                    };
                    Ok(Some(
                        inbox
                            .resolve_report(reference)
                            .await
                            .map_err(StoreError::backend)?
                            .outcome,
                    ))
                })
            })
            .await
            .unwrap()
    }
    pub async fn sql(&self, statement: &'static str) {
        self.store
            .transaction(move |scope| {
                Box::pin(async move {
                    sqlx::raw_sql(statement).execute(scope.executor()).await?;
                    Ok(())
                })
            })
            .await
            .unwrap();
    }
    pub async fn counts(&self) -> Vec<i64> {
        self.store
            .transaction(|scope| {
                Box::pin(async move {
                    let mut counts = Vec::new();
                    for statement in [
                        "SELECT COUNT(*) AS count FROM _fleet_sync_inbox_items",
                        "SELECT COUNT(*) AS count FROM router_pushes",
                        "SELECT COUNT(*) AS count FROM _fleet_sync_reports",
                        "SELECT COUNT(*) AS count FROM message_send_answers",
                    ] {
                        counts.push(
                            sqlx::query::<ProjectionDatabase>(statement)
                                .fetch_one(scope.executor())
                                .await?
                                .try_get("count")?,
                        );
                    }
                    Ok(counts)
                })
            })
            .await
            .unwrap()
    }
    pub async fn local_outcome(&self, item: ItemId) -> Option<RecipientOutcome<CarrierAcceptance>> {
        let config = self.host.config;
        self.store
            .transaction(move |scope| {
                Box::pin(async move {
                    Inbox::<_, RouterDeliveryTypes>::new(
                        scope,
                        config.local.owner,
                        config.local.limits,
                    )
                    .local_outcome(item)
                    .await
                    .map_err(StoreError::backend)
                })
            })
            .await
            .unwrap()
    }
    pub fn push_answer(&self, outcome: LocalOutcome<CarrierAcceptance>, receipt: DeliveryReceipt) {
        let scheduled_retry_at = match &outcome {
            LocalOutcome::Held { until } => *until,
            _ => None,
        };
        self.host
            .consumer
            .answers
            .lock()
            .unwrap()
            .push_back(LocalDelivery {
                outcome,
                observation: crate::RouterLocalObservation {
                    receipt,
                    scheduled_retry_at,
                    route_evidence: None,
                },
            });
    }
    /// The external receipt stands in; the Inbox, journal and projection stay real.
    pub async fn settle_with_peer_receipt(&self, draft: PushRecordDraft) -> ItemId {
        let item = self.item(&draft).id();
        assert!(self.admit_local(draft).await.unwrap());
        self.push_answer(
            LocalOutcome::Delivered {
                acceptance: CarrierAcceptance::PeerMessageWritten,
            },
            DeliveryReceipt {
                outcome: DeliveryOutcome::PeerMessageWritten,
                reachability: Some(collaboration_protocol::SessionReachability::ClaudeCodePeer),
                client: None,
            },
        );
        let engine = self.engine();
        assert_eq!(engine.drive_due().await.unwrap().completed.len(), 1);
        drop(engine);
        item
    }
    pub async fn reopen(self) -> Self {
        let peer = self.host.config.local.owner;
        let directory = self.close().await;
        Self::open(directory, peer).await
    }
    pub async fn close(self) -> tempfile::TempDir {
        let Self {
            owner,
            store,
            host,
            directory,
        } = self;
        drop(store);
        drop(host);
        Arc::try_unwrap(owner)
            .unwrap_or_else(|_| panic!("projection owner retained"))
            .close()
            .await
            .unwrap();
        directory
    }
}
