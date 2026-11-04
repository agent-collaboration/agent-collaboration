//! Registry calls publish lease facts and discharge claim debt at the Fleet owner.
use crate::{
    CallerKind, FleetKind, ObjectAddress, ObjectHost, ObjectRuntime, PlatformIo,
    caller_context::CallAuthority,
    fleet_lease_step::StoreClaimGrant,
    lease_projection::{ClaimGrantedUpdate, LeaseProjectionResult},
    object_call_envelope::ObjectCallEnvelope,
    object_runtime::ObjectWork,
    step_driver::StepDriver,
};
use durable_steps::{
    AtomicStore, CallFailure, CallTarget, NotSentReason, storage::TransactionScope,
};
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

#[derive(Serialize, Deserialize)]
pub(crate) struct ClaimGranted;
impl CallTarget for ClaimGranted {
    const KIND: &'static str = "central.claim-granted";
    type Request = ClaimGrantedUpdate;
    type Answer = LeaseProjectionResult;
}

impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<FleetKind, Host, Io>
where
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'scalar> i64: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'scalar, Host::Database>
        + sqlx_core::decode::Decode<'scalar, Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub(crate) async fn claim_granted_call(
        &self,
        call: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        require_registry(&call)?;
        let _: ClaimGranted = serde_json::from_value(call.target).map_err(|_| refused())?;
        let update: ClaimGrantedUpdate =
            serde_json::from_value(call.request).map_err(|_| refused())?;
        let ObjectWork::Fleet { host, io, .. } = &self.work else {
            return Err(refused());
        };
        let (result, _) = StepDriver::new(host.clone())
            .run(StoreClaimGrant {
                update,
                call: call.identity,
                now: chrono::DateTime::from_timestamp_millis(io.now().unix_millis())
                    .ok_or(CallFailure::AnswerLost)?,
                row_ids: crate::fleet_row_ids::FleetRowIds::new(io.clone()),
            })
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        serde_json::to_vec(&result).map_err(|_| CallFailure::AnswerLost)
    }
}

fn require_registry(call: &ObjectCallEnvelope) -> Result<(), CallFailure> {
    if call.source == ObjectAddress::Fleet
        && call.source_face == CallerKind::Registry
        && matches!(call.authority, CallAuthority::Object)
    {
        Ok(())
    } else {
        Err(refused())
    }
}
fn refused() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}
