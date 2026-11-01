//! The Fleet face owns central's nonreplicated application state.

mod automation_calls;
mod automation_catalog_step;
mod automation_clock_step;

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
        authoritative_receiver: fleet_sync_protocol::ReceiverId,
    ) -> Result<Self, StoreError> {
        let io = Arc::new(io);
        let shared_host = Arc::new(CoreHost::fleet(io.clone(), authoritative_receiver));
        let engine =
            crate::shared_task_registry::fleet_engine(Arc::clone(host.store()), shared_host)?;
        Ok(Self {
            work: ObjectWork::Fleet { host, io, engine },
            face: PhantomData,
        })
    }
    pub async fn on_alarm(&self) -> Result<durable_steps::DriveReport, StoreError> {
        let ObjectWork::Fleet { engine, host, io } = &self.work else {
            return Err(wrong_face());
        };
        self.prune_feed().await?;
        StepDriver::new(host.clone())
            .run(automation_clock_step::EvaluateAutomations {
                now_ms: io.now().unix_millis(),
                row_ids: crate::fleet_row_ids::FleetRowIds::new(io.clone()),
            })
            .await?;
        let driven = engine.drive_due().await?;
        self.rearm().await?;
        Ok(driven)
    }
    pub async fn initialize(&self) -> Result<usize, StoreError> {
        let ObjectWork::Fleet { host, io, .. } = &self.work else {
            return Err(wrong_face());
        };
        let applied = ObjectBootstrap::new(host.clone(), io.now())
            .initialize(EngineSchema::Fleet)
            .await?;
        self.prune_feed().await?;
        Ok(applied)
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
        let call = ObjectCallEnvelope::decode(&bytes, ObjectAddress::Fleet, CallerKind::Fleet);
        let automation_call = call
            .as_ref()
            .is_ok_and(|call| call.kind == crate::fleet_automation_calls::FleetAutomation::KIND);
        // Raw debug observations must not repair the very alarm being observed.
        // Normal requests and the existing journal fixture keep their postlude.
        #[cfg(all(feature = "conformance", debug_assertions))]
        let fixture_only = call.as_ref().is_ok_and(|call| {
            call.kind == crate::conformance_fleet::FleetReceiptProbe::KIND
                && matches!(
                    serde_json::from_value::<crate::StepControl>(call.request.clone()),
                    Ok(crate::StepControl::AutomationFixture { .. })
                )
        });
        #[cfg(not(all(feature = "conformance", debug_assertions)))]
        let fixture_only = false;
        let answer = match call {
            Ok(call) => self.dispatch_call(call).await,
            Err(error) => Err(error),
        };
        if !fixture_only {
            let rearmed = if automation_call {
                self.rearm().await
            } else {
                self.rearm_journal().await
            };
            rearmed.map_err(|_| CallFailure::AnswerLost)?;
        }
        answer
    }
    async fn rearm_journal(&self) -> Result<(), StoreError> {
        let ObjectWork::Fleet { host, .. } = &self.work else {
            return Err(wrong_face());
        };
        // Unrelated Fleet calls restore their journal debt without scanning the
        // Automation catalog. Both platform timers preserve an earlier wake;
        // catalog calls, machine pulls and actual alarms restore that deadline.
        let earliest = host
            .store()
            .transaction(|scope| Box::pin(async move { scope.earliest().await }))
            .await?;
        if let Some(at) = earliest {
            host.wake().wake_at(at).await.map_err(StoreError::backend)?;
        }
        Ok(())
    }
    async fn rearm(&self) -> Result<(), StoreError> {
        let ObjectWork::Fleet { host, .. } = &self.work else {
            return Err(wrong_face());
        };
        // The mutation saved both notice debt and wake intent in its transaction.
        // Only after it settles do we arm the platform timer. An inbound call
        // never drives notice work; a replay can also rearm an earlier saved debt.
        let earliest = host
            .store()
            .transaction(|scope| Box::pin(crate::automation_clock_deadline::request_next(scope)))
            .await?;
        if let Some(at) = earliest {
            host.wake().wake_at(at).await.map_err(StoreError::backend)?;
        }
        Ok(())
    }

    async fn dispatch_call(&self, call: ObjectCallEnvelope) -> Result<Vec<u8>, CallFailure> {
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
        if call.kind == crate::fleet_automation_calls::FleetAutomation::KIND {
            return self.automation_call(call).await;
        }
        if call.kind == crate::fleet_lease_calls::AdvanceLeaseIncarnation::KIND {
            return self.advance_lease_incarnation_call(call).await;
        }
        if call.kind == crate::fleet_lease_calls::ClaimGranted::KIND {
            return self.claim_granted_call(call).await;
        }
        if call.kind == crate::fleet_lease_calls::PublishLeaseProjection::KIND {
            return self.publish_lease_projection_call(call).await;
        }
        if call.kind == crate::admin_import_fleet::AdminImportWorkflow::KIND {
            return self.admin_import_workflow(call).await;
        }
        #[cfg(feature = "conformance")]
        if call.kind == crate::conformance_fleet::FleetReceiptProbe::KIND {
            return self.receipt_probe(call).await;
        }
        if !matches!(call.authority, CallAuthority::Object)
            || call.source_face != CallerKind::Session
            || !matches!(call.source, ObjectAddress::Connection(_))
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
