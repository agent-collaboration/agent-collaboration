//! Only Registry object calls may reserve or settle a native claim attempt.
use crate::{
    CallerKind, FleetKind, ObjectAddress, ObjectHost, ObjectRuntime, PlatformIo,
    caller_context::CallAuthority,
    fleet_claim_step::{ClaimStep, ClaimStepResult},
    object_call_envelope::ObjectCallEnvelope,
    object_runtime::ObjectWork,
    step_driver::StepDriver,
};
use durable_steps::{
    AtomicStore, CallFailure, CallTarget, NotSentReason, storage::TransactionScope,
};
use fleet_sync_protocol::{HolderChangeRequest, SettleOutcome, SettlementRecorded};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

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
    pub(crate) async fn claim_gate_call(
        &self,
        call: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        if call.source != ObjectAddress::Registry
            || call.source_face != CallerKind::Registry
            || !matches!(call.authority, CallAuthority::Object)
        {
            return Err(refused());
        }
        let ObjectWork::Fleet { host, io, .. } = &self.work else {
            return Err(refused());
        };
        let step = if call.kind == fleet_sync_lease::ClaimReservation::KIND {
            let _: fleet_sync_lease::ClaimReservation =
                serde_json::from_value(call.target).map_err(|_| refused())?;
            let request: fleet_sync_protocol::ClaimReservation =
                serde_json::from_value(call.request).map_err(|_| refused())?;
            let HolderChangeRequest::Claim {
                database, attempt, ..
            } = request.change
            else {
                return Err(refused());
            };
            ClaimStep::Reserve { database, attempt }
        } else {
            let _: fleet_sync_lease::ClaimSettlement =
                serde_json::from_value(call.target).map_err(|_| refused())?;
            let request: fleet_sync_protocol::ClaimSettlement =
                serde_json::from_value(call.request).map_err(|_| refused())?;
            let HolderChangeRequest::Claim {
                database, attempt, ..
            } = request.change
            else {
                return Err(refused());
            };
            match request.outcome {
                SettleOutcome::Cancelled | SettleOutcome::Refused { .. } => ClaimStep::Refuse {
                    database,
                    attempt,
                    now: chrono::DateTime::from_timestamp_millis(io.now().unix_millis())
                        .ok_or(CallFailure::AnswerLost)?,
                    row_ids: crate::fleet_row_ids::FleetRowIds::new(io.clone()),
                },
                SettleOutcome::Granted { .. } => return Err(refused()),
            }
        };
        let (answer, _) = StepDriver::new(host.clone())
            .run(step)
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        match answer {
            ClaimStepResult::Reservation(answer) => serde_json::to_vec(&answer),
            ClaimStepResult::Settled => serde_json::to_vec(&SettlementRecorded),
        }
        .map_err(|_| CallFailure::AnswerLost)
    }
}
fn refused() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}
