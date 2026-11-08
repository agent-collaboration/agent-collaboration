//! The Fleet face owns central's nonreplicated application state.
mod notice_push;
mod request_send;
use crate::object_event::{EventReply, ObjectEvent};
use runtime_primitives::{Clock as _, IdSource as _};

use crate::{
    CallerKind, CoreHost, EngineSchema, FleetKind, ObjectAddress, ObjectBootstrap, ObjectPlatform,
    ObjectRuntime,
    caller_context::CallAuthority,
    fleet_effects_read::ReadFleetEffects,
    object_call_envelope::{ObjectCallEnvelope, encode_refusal},
    object_runtime::ObjectWork,
    step_driver::StepDriver,
};
use crate::{
    mail_sender_calls::{AdmitSenderMail, SenderFleetMail, SenderMailRequest, SenderRoute},
    mail_submit_admission::AdmitMailSubmit,
    mail_submit_calls::{FleetMailSubmit, MailSubmitRequest},
};
use durable_steps::{
    AtomicStore, CallFailure, CallTarget, NotSentReason, StoreError, storage::TransactionScope,
};
use message_board::RequestEffectsRequest;
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::{marker::PhantomData, sync::Arc};

#[derive(Serialize, Deserialize)]
pub(crate) struct FleetRequestEffects;
impl CallTarget for FleetRequestEffects {
    const KIND: &'static str = "central.fleet-request-effects";
    type Request = RequestEffectsRequest;
    type Answer = board_sync_protocol::BoardReadPage;
}

impl<Host: ObjectPlatform> ObjectRuntime<FleetKind, Host>
where
    Host::Store: durable_steps::ReborrowScope,
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'scalar> i64: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'scalar, Host::Database>
        + sqlx_core::decode::Decode<'scalar, Host::Database>,
    for<'blob> Vec<u8>: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'blob, Host::Database>
        + sqlx_core::decode::Decode<'blob, Host::Database>,
    for<'blob> Option<Vec<u8>>:
        sqlx_core::types::Type<Host::Database> + sqlx_core::decode::Decode<'blob, Host::Database>,
    for<'scalar> Option<i64>:
        sqlx_core::types::Type<Host::Database> + sqlx_core::decode::Decode<'scalar, Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub fn fleet(
        host: Arc<Host>,
        config: fleet_project_lease::RegistryConfig,
        delivery: crate::DeliveryConfig,
        holder_answer_timeout: std::time::Duration,
        honored_capabilities: fleet_protocol::FrameCapabilities,
    ) -> Result<Self, StoreError> {
        let io = host.io().clone();
        let replies = Arc::new(crate::holder_reply_hub::HolderReplyHub::default());
        let notice_cursors = Arc::new(crate::notice_push::NoticeCursors::default());
        let shared_host = Arc::new(CoreHost::registry(
            io.clone(),
            config.authoritative_receiver,
            crate::registry_peer_lookup::bind(Arc::downgrade(&host)),
            Some(crate::registry_lease_store::bind(Arc::downgrade(&host))),
        ));
        let store = Arc::new(crate::session_store::CoreSessionStore::new(
            host.clone(),
            shared_host.clone(),
            replies.clone(),
        ));
        type LeaseHook<Host> = crate::fleet_lease_projection_step::FleetLeaseChangeHook<
            crate::session_store::CoreSessionStore<Host>,
        >;
        let hook = Arc::new(LeaseHook::<Host>::new(io.clone()));
        let mut engine = crate::shared_task_registry::registry_engine::<_, Host::Database, _, _>(
            store,
            shared_host.clone(),
            config,
            hook,
        )?;
        engine.register(fleet_session::PublishSchemaSupport::<
            crate::session_store::CoreSessionStore<Host>,
            Host::Database,
        >::default())?;
        engine.register(crate::session_revocation::EndSessionsTask::<Host>(
            PhantomData,
        ))?;
        let engine =
            crate::fleet_task_engine::own_engine::<_, _, Host::Database, LeaseHook<Host>>(engine);
        let weak_host = Arc::downgrade(&host);
        let weak_shared = Arc::downgrade(&shared_host);
        let weak_engine = Arc::downgrade(&engine);
        let local_io = io.clone();
        let local_replies = replies.clone();
        let local_cursors = notice_cursors.clone();
        shared_host.install_local_calls(Arc::new(move |bytes| {
            let host = weak_host.clone();
            let shared = weak_shared.clone();
            let engine = weak_engine.clone();
            let io = local_io.clone();
            let replies = local_replies.clone();
            let notice_cursors = local_cursors.clone();
            Box::pin(async move {
                let runtime = ObjectRuntime::<FleetKind, Host> {
                    work: ObjectWork::Fleet {
                        host: host.upgrade().ok_or(CallFailure::AnswerLost)?,
                        shared_host: shared.upgrade().ok_or(CallFailure::AnswerLost)?,
                        engine: engine.upgrade().ok_or(CallFailure::AnswerLost)?,
                        io,
                        replies,
                        config,
                        delivery,
                        holder_answer_timeout,
                        honored_capabilities,
                        notice_cursors,
                    },
                    face: PhantomData,
                };
                runtime.on_call(bytes).await
            })
        }))?;
        Ok(Self {
            work: ObjectWork::Fleet {
                host,
                io,
                engine,
                shared_host,
                replies,
                config,
                delivery,
                holder_answer_timeout,
                honored_capabilities,
                notice_cursors,
            },
            face: PhantomData,
        })
    }
    /// The event's own handling, before its notices are pushed.
    pub(crate) async fn handle_fleet_event(&self, event: ObjectEvent) -> EventReply {
        match event {
            ObjectEvent::SocketOpened { binding } => match self.on_socket_opened(binding).await {
                Ok(()) => EventReply::None,
                Err(failure) => {
                    eprintln!("Fleet socket open failed: {failure}");
                    EventReply::SocketFailed
                }
            },
            ObjectEvent::Frame { binding, frame } => match self.on_frame(binding, frame).await {
                Ok(()) => EventReply::None,
                Err(failure) => {
                    eprintln!("Fleet frame failed: {failure}");
                    EventReply::SocketFailed
                }
            },
            ObjectEvent::SocketClosed { binding } => {
                if let Err(failure) = self.on_socket_closed(binding).await {
                    eprintln!("Fleet socket close failed: {failure}");
                }
                EventReply::None
            }
            ObjectEvent::Call { body, .. } => match self.on_call(body).await {
                Ok(answer) => EventReply::CallAnswer(answer),
                Err(failure) => EventReply::CallFailure(failure),
            },
            ObjectEvent::Alarm => EventReply::AlarmDone,
        }
    }

    /// Every Fleet due source except long operations, each failing alone.
    pub(crate) async fn run_fleet_due_sources(&self, now: runtime_primitives::SyncTime) {
        self.run_feed_retention(now).await;
        self.run_mail_retention(now).await;
        self.run_push_expiry(now).await;
        self.resend_notices(now).await;
        self.resend_due_requests(now).await;
        let ObjectWork::Fleet { io, .. } = &self.work else {
            return;
        };
        let (store, _) = match self.session_store() {
            Ok(handles) => handles,
            Err(failure) => {
                eprintln!("Fleet session store unavailable for due work: {failure}");
                return;
            }
        };
        // Each machine's stream is its own unit with its own attempt row.
        if let Err(failure) =
            crate::machine_notice_store::prune::<_, Host::Database>(store, now, io.as_ref()).await
        {
            eprintln!("Fleet machine notice maintenance could not list machines: {failure}");
        }
    }

    /// The Fleet object's subject for failures with no row of their own: the
    /// feed state row's id, which step 0 creates.
    pub(crate) async fn fleet_subject(&self) -> Result<durable_steps::Uuid, StoreError> {
        let ObjectWork::Fleet { host, .. } = &self.work else {
            return Err(wrong_face());
        };
        host.store()
            .transaction(|scope| {
                Box::pin(async move { crate::fleet_feed_store::state_subject(scope).await })
            })
            .await
    }

    /// Feed retention as one unit, backing off while it fails.
    async fn run_feed_retention(&self, now: runtime_primitives::SyncTime) {
        let ObjectWork::Fleet { host, io, .. } = &self.work else {
            return;
        };
        let subject = match self.fleet_subject().await {
            Ok(subject) => subject,
            Err(failure) => {
                eprintln!("Fleet feed retention has no subject: {failure}");
                return;
            }
        };
        let Some(cutoff_clock) = chrono::DateTime::from_timestamp_millis(now.unix_millis()) else {
            eprintln!("Fleet feed retention clock is out of range");
            return;
        };
        crate::due_unit::run_due_unit(
            host.store().as_ref(),
            crate::due_unit::DueItem {
                subject,
                kind: crate::work_attempts::AttemptKind::FeedRetention,
                now,
                attempt_id: io.new_id(),
            },
            move |scope| Box::pin(crate::fleet_feed_store::prune(scope, cutoff_clock)),
        )
        .await;
    }

    /// Mail records whose 60-day retention ended are removed, at most 100 per
    /// batch; a full batch leaves a past deadline, so the next batch follows.
    async fn run_mail_retention(&self, now: runtime_primitives::SyncTime) {
        let ObjectWork::Fleet { host, io, .. } = &self.work else {
            return;
        };
        let subject = match self.fleet_subject().await {
            Ok(subject) => subject,
            Err(failure) => {
                eprintln!("Fleet mail retention has no subject: {failure}");
                return;
            }
        };
        crate::due_unit::run_due_unit(
            host.store().as_ref(),
            crate::due_unit::DueItem {
                subject,
                kind: crate::work_attempts::AttemptKind::MailRetention,
                now,
                attempt_id: io.new_id(),
            },
            move |scope| {
                Box::pin(async move {
                    fleet_mail_delivery::remove_expired_relay_records(scope, now, 100)
                        .await
                        .map(|_| ())
                        .map_err(StoreError::backend)
                })
            },
        )
        .await;
    }

    /// Forwarded posts past their 30 s deadline end Unanswered and free their slot.
    /// Never held by the cadence: the deadline is a protocol answer. While it
    /// fails, it backs off on the Fleet object's forward_expiry row.
    pub(crate) async fn expire_fleet_forwards(&self, now: runtime_primitives::SyncTime) {
        let ObjectWork::Fleet { io, .. } = &self.work else {
            return;
        };
        let store = match self.session_store() {
            Ok((store, _)) => store,
            Err(failure) => {
                eprintln!("Fleet session store unavailable for forward expiry: {failure}");
                return;
            }
        };
        let subject = match self.fleet_subject().await {
            Ok(subject) => subject,
            Err(failure) => {
                eprintln!("Fleet forward expiry has no subject: {failure}");
                return;
            }
        };
        crate::due_unit::run_due_unit(
            store.as_ref(),
            crate::due_unit::DueItem {
                subject,
                kind: crate::work_attempts::AttemptKind::ForwardExpiry,
                now,
                attempt_id: io.new_id(),
            },
            move |scope| Box::pin(async move { scope.expire_forwards(now).await }),
        )
        .await;
    }

    /// Step 0 for Fleet. A failure is kept on the object's open row when
    /// storage can still be written, and only logged otherwise.
    pub(crate) async fn open_fleet(&self) -> Result<(), StoreError> {
        let ObjectWork::Fleet { host, io, .. } = &self.work else {
            return Err(wrong_face());
        };
        let opened = self.initialize().await.map(|_applied| ());
        let subject = match self.fleet_subject().await {
            Ok(subject) => subject,
            Err(missing) => {
                if let Err(failure) = &opened {
                    eprintln!("Fleet open failed before it had a subject ({missing}): {failure}");
                }
                return opened;
            }
        };
        let now = io.now();
        let attempt_id = io.new_id();
        let failure = opened
            .as_ref()
            .err()
            .map(crate::work_attempts::AttemptFailure::from);
        let recorded = host
            .store()
            .transaction(move |scope| {
                Box::pin(async move {
                    let kind = crate::work_attempts::AttemptKind::Open;
                    match failure {
                        None => crate::work_attempts::succeeded(scope, subject, kind).await,
                        Some(failure) => crate::work_attempts::failed(
                            scope, attempt_id, subject, kind, &failure, now,
                        )
                        .await
                        .map(|_| ()),
                    }
                })
            })
            .await;
        if let Err(unwritten) = recorded {
            eprintln!("Fleet open row could not be written: {unwritten}");
        }
        opened
    }

    /// The long-operation engine. Until slice 4 it still carries every job kind.
    pub(crate) async fn drive_fleet_operations(&self) {
        if let Err(failure) = self.drive_fleet_engine().await {
            eprintln!("Fleet job drive failed: {failure}");
        }
    }

    /// One engine drive and its report, for conformance checks of job outcomes.
    #[cfg(feature = "conformance")]
    pub async fn conformance_drive_operations(
        &self,
    ) -> Result<durable_steps::DriveReport, StoreError> {
        self.drive_fleet_engine().await
    }

    async fn drive_fleet_engine(&self) -> Result<durable_steps::DriveReport, StoreError> {
        let ObjectWork::Fleet { engine, .. } = &self.work else {
            return Err(wrong_face());
        };
        // Callers combine this drive with socket work. Keep its phases in one
        // heap-owned future rather than inflating every caller's frame.
        Box::pin(engine.drive_due()).await
    }
    pub async fn initialize(&self) -> Result<usize, StoreError> {
        let ObjectWork::Fleet { host, io, .. } = &self.work else {
            return Err(wrong_face());
        };
        let bootstrap = ObjectBootstrap::new(host.clone(), io.clone());
        let mut applied = 0;
        for schema in [
            EngineSchema::Fleet,
            EngineSchema::Registry,
            EngineSchema::Session,
            EngineSchema::Delivery,
        ] {
            applied += bootstrap.initialize(schema).await?;
        }
        // Feed retention is its own due source; opening never prunes, but it
        // creates the feed state row that append and failures rely on (#pipeline step 0).
        let now = chrono::DateTime::from_timestamp_millis(io.now().unix_millis())
            .ok_or_else(|| StoreError::backend("invalid Fleet open clock"))?;
        StepDriver::new(host.clone())
            .run(crate::fleet_feed_maintenance::EnsureFeedState {
                id: io.new_id(),
                now,
            })
            .await?;
        // After migrations, before any event work: what was issued until now
        // is not this runtime's to push.
        self.seed_notice_cursors().await;
        Ok(applied)
    }
    #[cfg(feature = "conformance")]
    pub async fn conformance_schema(&self) -> Result<Vec<crate::FleetSchemaObject>, StoreError> {
        let ObjectWork::Fleet { host, .. } = &self.work else {
            return Err(wrong_face());
        };
        host.store()
            .transaction(|scope| {
                Box::pin(async move { crate::fleet_schema_observation::read_schema(scope).await })
            })
            .await
    }
    #[cfg(feature = "conformance")]
    pub async fn conformance_mail_custody(
        &self,
        item: fleet_protocol::ItemId,
    ) -> Result<crate::FleetMailCustodyObservation, StoreError> {
        let ObjectWork::Fleet { host, .. } = &self.work else {
            return Err(wrong_face());
        };
        host.store()
            .transaction(move |scope| {
                Box::pin(
                    async move { crate::fleet_mail_observation::read_custody(scope, item).await },
                )
            })
            .await
    }
    pub async fn on_call(&self, bytes: Vec<u8>) -> Result<Vec<u8>, CallFailure> {
        let envelope: ObjectCallEnvelope = serde_json::from_slice(&bytes).map_err(|_| refused())?;
        match envelope.destination_face {
            CallerKind::Registry => self.on_registry_call(bytes).await,
            CallerKind::Delivery => self.delivery().map_err(|_| refused())?.on_call(bytes).await,
            _ => self.dispatch_call(bytes).await,
        }
    }

    pub(crate) async fn mail_submit_call(
        &self,
        invocation: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        if invocation.source_face != CallerKind::Session
            || !matches!(invocation.authority, CallAuthority::Machine { .. })
        {
            return Err(refused());
        }
        let CallAuthority::Machine {
            installation: source,
        } = invocation.authority
        else {
            return Err(refused());
        };
        let _: FleetMailSubmit =
            serde_json::from_value(invocation.target).map_err(|_| refused())?;
        let request: MailSubmitRequest =
            serde_json::from_value(invocation.request).map_err(|_| refused())?;
        let ObjectWork::Fleet {
            host, io, delivery, ..
        } = &self.work
        else {
            return Err(refused());
        };
        let (answer, _) = StepDriver::new(host.clone())
            .run(AdmitMailSubmit {
                source,
                request,
                limits: crate::fleet_outbox_limits().map_err(|_| CallFailure::AnswerLost)?,
                relay: delivery.relay_peer,
                clock: io.clone(),
            })
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        serde_json::to_vec(&answer).map_err(|_| CallFailure::AnswerLost)
    }

    pub(crate) async fn sender_mail_call(
        &self,
        invocation: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        if invocation.source_face != CallerKind::Session
            || !matches!(invocation.authority, CallAuthority::Machine { .. })
        {
            return Err(refused());
        }
        let CallAuthority::Machine {
            installation: source,
        } = invocation.authority
        else {
            return Err(refused());
        };
        let _: SenderFleetMail =
            serde_json::from_value(invocation.target).map_err(encode_refusal)?;
        let request: SenderMailRequest =
            serde_json::from_value(invocation.request).map_err(encode_refusal)?;
        if request.request.call() != &invocation.identity {
            return Err(refused());
        }
        let ObjectWork::Fleet { host, io, .. } = &self.work else {
            return Err(refused());
        };
        let (route, _) = StepDriver::new(host.clone())
            .run(AdmitSenderMail {
                source,
                input: request,
                limits: crate::fleet_outbox_limits().map_err(|_| CallFailure::AnswerLost)?,
                now: io.now(),
            })
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        let answer = match route {
            SenderRoute::Refused => return Err(refused()),
            SenderRoute::Answered(answer) => answer,
        };
        serde_json::to_vec(&answer).map_err(|_| CallFailure::AnswerLost)
    }

    async fn dispatch_call(&self, bytes: Vec<u8>) -> Result<Vec<u8>, CallFailure> {
        let call = ObjectCallEnvelope::decode(&bytes, ObjectAddress::Fleet, CallerKind::Fleet)?;
        if call.kind == crate::mail_submit_calls::FleetMailSubmit::KIND {
            return self.mail_submit_call(call).await;
        }
        if call.kind == crate::mail_sender_calls::SenderFleetMail::KIND {
            return self.sender_mail_call(call).await;
        }
        if matches!(
            call.kind.as_str(),
            crate::owner_operation_calls::FleetProjectOperations::KIND
                | crate::admin_import_copy::FleetAdminCopySeed::KIND
                | crate::copy_write_calls::FleetProjectSeed::KIND
        ) {
            return self.owner_project_call(call).await;
        }
        if call.kind == crate::owner_operation_calls::ReadCopyDirectoryPage::KIND {
            return self.owner_directory_page(call).await;
        }
        if call.kind == crate::fleet_creation_calls::ReadProjectCreationDecision::KIND {
            return self.creation_decision_call(call).await;
        }
        if call.kind == fleet_project_lease::RetirementAdmission::KIND {
            return self.retirement_gate_call(call).await;
        }
        if call.kind == crate::fleet_archive_calls::FleetArchiveReport::KIND {
            return self.archive_report_call(call).await;
        }
        if call.kind == fleet_project_lease::ClaimReservation::KIND
            || call.kind == fleet_project_lease::ClaimSettlement::KIND
        {
            return self.claim_gate_call(call).await;
        }
        if call.kind == crate::fleet_read_calls::FleetRead::KIND {
            return self.fleet_read_call(call).await;
        }
        if call.kind == crate::copy_directory_summary::DirectorySummaryUpdate::KIND {
            return self.directory_summary_call(call).await;
        }
        if call.kind == "central.object" {
            return self.import_call(call).await;
        }
        if call.kind == crate::fleet_metadata_calls::FleetMetadata::KIND {
            return self.metadata_call(call).await;
        }
        if call.kind == crate::fleet_lease_calls::ClaimGranted::KIND {
            return self.claim_granted_call(call).await;
        }
        if call.kind == crate::admin_import_fleet::AdminImportWorkflow::KIND {
            return self.admin_import_workflow(call).await;
        }
        #[cfg(feature = "conformance")]
        if call.kind == crate::conformance_fleet::FleetReceiptProbe::KIND {
            return self.receipt_probe(call).await;
        }
        if !matches!(call.authority, CallAuthority::Machine { .. })
            || call.source_face != CallerKind::Session
            || !matches!(call.source, ObjectAddress::Fleet)
            || call.kind != FleetRequestEffects::KIND
        {
            return Err(refused());
        }
        let _: FleetRequestEffects = serde_json::from_value(call.target).map_err(|_| refused())?;
        let request = serde_json::from_value(call.request).map_err(|_| refused())?;
        let ObjectWork::Fleet { host, io, .. } = &self.work else {
            return Err(refused());
        };
        let as_of = chrono::DateTime::from_timestamp_millis(io.now().unix_millis())
            .ok_or(CallFailure::AnswerLost)?;
        let (result, _) = StepDriver::new(host.clone())
            .run(ReadFleetEffects(request))
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        serde_json::to_vec(&board_sync_protocol::BoardReadPage::RequestEffects { result, as_of })
            .map_err(|_| CallFailure::AnswerLost)
    }
}
fn wrong_face() -> StoreError {
    StoreError::Aborted {
        reason: "Fleet request reached a different object face".into(),
    }
}
fn refused() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}
