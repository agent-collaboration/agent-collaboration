//! The Fleet face owns central's nonreplicated application state.

use crate::{
    CallerKind, CoreHost, EngineSchema, FleetKind, ObjectAddress, ObjectBootstrap, ObjectHost,
    ObjectRuntime, PlatformIo, caller_context::CallAuthority, fleet_effects_read::ReadFleetEffects,
    object_call_envelope::ObjectCallEnvelope, object_runtime::ObjectWork, step_driver::StepDriver,
};
use durable_steps::{
    AtomicStore, CallFailure, CallTarget, NotSentReason, StoreError,
    storage::{JournalStorage, TransactionScope},
};
use message_board::RequestEffectsRequest;
use runtime_primitives::WakeScheduler;
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

impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<FleetKind, Host, Io>
where
    Host::Store: durable_steps::ReborrowScope,
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub fn fleet(
        host: Arc<Host>,
        io: Io,
        config: fleet_sync_lease::RegistryConfig,
        delivery: crate::DeliveryConfig,
        holder_answer_timeout: std::time::Duration,
    ) -> Result<Self, StoreError> {
        let io = Arc::new(io);
        let replies = Arc::new(crate::holder_reply_hub::HolderReplyHub::default());
        let mail_limits = crate::central_mailbox_limits().map_err(StoreError::backend)?;
        let publication = crate::mail_status_dispatch::bind(
            Arc::downgrade(&host),
            io.clone(),
            replies.clone(),
            holder_answer_timeout,
        );
        let shared_host = Arc::new(
            CoreHost::registry(
                io.clone(),
                config.authoritative_receiver,
                crate::registry_peer_lookup::bind(Arc::downgrade(&host)),
                Some(crate::registry_lease_store::bind(Arc::downgrade(&host))),
            )
            .with_publication(publication)
            .with_push(crate::mail_push_dispatch::bind(
                Arc::downgrade(&host),
                io.clone(),
                replies.clone(),
                mail_limits,
                holder_answer_timeout,
            )),
        );
        let store = Arc::new(crate::session_store::CoreSessionStore::new(
            host.clone(),
            shared_host.clone(),
            replies.clone(),
        ));
        type NoticeTask<Host, Io> = crate::machine_notice_delivery::MachineNoticeTask<Host, Io>;
        type LeaseHook<Host, Io> = crate::fleet_lease_projection_step::FleetLeaseChangeHook<
            crate::session_store::CoreSessionStore<Host, Io>,
            NoticeTask<Host, Io>,
        >;
        let hook = Arc::new(LeaseHook::<Host, Io>::new(io.clone()));
        let mut engine = crate::shared_task_registry::registry_engine::<_, Host::Database, _, _>(
            store,
            shared_host.clone(),
            config,
            hook,
        )?;
        engine.register(crate::machine_notice_outbox::FleetNoticeTask::<
            crate::session_store::CoreSessionStore<Host, Io>,
        >(PhantomData))?;
        engine
            .register(crate::machine_notice_delivery::MachineNoticeTask::<Host, Io>(PhantomData))?;
        engine.register(fleet_sync_session::PublishSchemaSupport::<
            crate::session_store::CoreSessionStore<Host, Io>,
            Host::Database,
        >::default())?;
        engine.register(crate::session_revocation::EndSessionsTask::<Host, Io>(
            PhantomData,
        ))?;
        engine.register(fleet_sync_delivery::PublishStatus::<
            crate::session_store::CoreSessionStore<Host, Io>,
            Host::Database,
            board_sync_protocol::BoardDeliveryTypes,
        >::new())?;
        engine.register(fleet_sync_delivery::PushItem::<
            crate::session_store::CoreSessionStore<Host, Io>,
            Host::Database,
            board_sync_protocol::BoardDeliveryTypes,
            fleet_sync_delivery::PublishStatus<
                crate::session_store::CoreSessionStore<Host, Io>,
                Host::Database,
                board_sync_protocol::BoardDeliveryTypes,
            >,
        >::new(mail_limits, shared_host.clone()))?;
        engine.register(fleet_sync_delivery::RemoveDeliveryRecord::<
            crate::session_store::CoreSessionStore<Host, Io>,
            Host::Database,
        >::new())?;
        let engine =
            crate::fleet_task_engine::own_engine::<_, _, Host::Database, LeaseHook<Host, Io>>(
                engine,
            );
        let weak_host = Arc::downgrade(&host);
        let weak_shared = Arc::downgrade(&shared_host);
        let weak_engine = Arc::downgrade(&engine);
        let local_io = io.clone();
        let local_replies = replies.clone();
        shared_host.install_local_calls(Arc::new(move |bytes| {
            let host = weak_host.clone();
            let shared = weak_shared.clone();
            let engine = weak_engine.clone();
            let io = local_io.clone();
            let replies = local_replies.clone();
            Box::pin(async move {
                let runtime = ObjectRuntime::<FleetKind, Host, Io> {
                    work: ObjectWork::Fleet {
                        host: host.upgrade().ok_or(CallFailure::AnswerLost)?,
                        shared_host: shared.upgrade().ok_or(CallFailure::AnswerLost)?,
                        engine: engine.upgrade().ok_or(CallFailure::AnswerLost)?,
                        io,
                        replies,
                        config,
                        delivery,
                        holder_answer_timeout,
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
            },
            face: PhantomData,
        })
    }
    pub async fn on_alarm(&self) -> Result<durable_steps::DriveReport, StoreError> {
        let ObjectWork::Fleet {
            engine,
            host,
            replies,
            shared_host,
            ..
        } = &self.work
        else {
            return Err(wrong_face());
        };
        self.prune_feed().await?;
        let now = runtime_primitives::Clock::now(shared_host.as_ref());
        replies.expire(now);
        if let Some(at) = replies.next_deadline() {
            host.wake().wake_at(at).await.map_err(StoreError::backend)?;
        }
        let (store, shared) = self.session_store()?;
        let now = runtime_primitives::Clock::now(shared.as_ref());
        store
            .transaction(move |scope| {
                Box::pin(async move {
                    scope.expire_forwards(now).await?;
                    crate::machine_notice_store::prune(scope, now).await?;
                    if let Some(at) = crate::machine_notice_store::next_expiry(scope).await? {
                        durable_steps::JournalScope::ensure_wake(scope, at).await?;
                    }
                    Ok(())
                })
            })
            .await?;
        let report = engine.drive_due().await?;
        Ok(report)
    }
    pub async fn initialize(&self) -> Result<usize, StoreError> {
        let ObjectWork::Fleet { host, io, .. } = &self.work else {
            return Err(wrong_face());
        };
        let bootstrap = ObjectBootstrap::new(host.clone(), io.now());
        let mut applied = 0;
        for schema in [
            EngineSchema::Fleet,
            EngineSchema::Registry,
            EngineSchema::Session,
            EngineSchema::Mailbox,
        ] {
            applied += bootstrap.initialize(schema).await?;
        }
        self.prune_feed().await?;
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
    async fn prune_feed(&self) -> Result<(), StoreError> {
        let ObjectWork::Fleet { host, io, .. } = &self.work else {
            return Err(wrong_face());
        };
        let now = chrono::DateTime::from_timestamp_millis(io.now().unix_millis())
            .ok_or_else(|| StoreError::backend("invalid Fleet retention clock"))?;
        StepDriver::new(host.clone())
            .run(crate::fleet_feed_maintenance::PruneFeed { now })
            .await?;
        Ok(())
    }
    pub async fn on_call(&self, bytes: Vec<u8>) -> Result<Vec<u8>, CallFailure> {
        let envelope: ObjectCallEnvelope = serde_json::from_slice(&bytes).map_err(|_| refused())?;
        let answer = match envelope.destination_face {
            CallerKind::Registry => self.on_registry_call(bytes).await,
            CallerKind::Delivery => self.delivery().map_err(|_| refused())?.on_call(bytes).await,
            _ => self.dispatch_call(bytes).await,
        };
        let ObjectWork::Fleet { host, .. } = &self.work else {
            return Err(refused());
        };
        // The mutation saved both notice debt and wake intent in its transaction.
        // Only after it settles do we arm the platform timer. An inbound call
        // never drives notice work; a replay can also rearm an earlier saved debt.
        let earliest = host
            .store()
            .transaction(|scope| Box::pin(async move { scope.earliest().await }))
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        if let Some(at) = earliest {
            host.wake()
                .wake_at(at)
                .await
                .map_err(|_| CallFailure::AnswerLost)?;
        }
        answer
    }

    async fn dispatch_call(&self, bytes: Vec<u8>) -> Result<Vec<u8>, CallFailure> {
        let call = ObjectCallEnvelope::decode(&bytes, ObjectAddress::Fleet, CallerKind::Fleet)?;
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
        if call.kind == fleet_sync_lease::RetirementAdmission::KIND {
            return self.retirement_gate_call(call).await;
        }
        if call.kind == crate::fleet_archive_calls::FleetArchiveReport::KIND {
            return self.archive_report_call(call).await;
        }
        if call.kind == fleet_sync_lease::ClaimReservation::KIND
            || call.kind == fleet_sync_lease::ClaimSettlement::KIND
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
