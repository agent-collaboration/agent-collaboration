//! Registry calls publish lease facts and discharge claim debt at the Fleet owner.
use crate::{
    CallerKind, FleetKind, ObjectAddress, ObjectHost, ObjectRuntime, PlatformIo,
    RegistryIncarnation,
    caller_context::CallAuthority,
    fleet_lease_projection_step::StoreLeaseProjection,
    fleet_lease_step::{AdvanceLeaseIncarnationStep, StoreClaimGrant},
    lease_projection::{ClaimGrantedUpdate, LeaseProjection, LeaseProjectionResult},
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
pub(crate) struct AdvanceLeaseIncarnation;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AdvanceLeaseIncarnationRequest {
    pub at_least: u64,
}
impl CallTarget for AdvanceLeaseIncarnation {
    const KIND: &'static str = "central.lease-incarnation";
    type Request = AdvanceLeaseIncarnationRequest;
    type Answer = RegistryIncarnation;
}

#[derive(Serialize, Deserialize)]
pub(crate) struct ClaimGranted;
impl CallTarget for ClaimGranted {
    const KIND: &'static str = "central.claim-granted";
    type Request = ClaimGrantedUpdate;
    type Answer = LeaseProjectionResult;
}

#[derive(Serialize, Deserialize)]
pub(crate) struct PublishLeaseProjection;
impl CallTarget for PublishLeaseProjection {
    const KIND: &'static str = "central.lease-projection";
    type Request = LeaseProjection;
    type Answer = LeaseProjectionResult;
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
    pub(crate) async fn publish_lease_projection_call(
        &self,
        call: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        require_registry(&call)?;
        let _: PublishLeaseProjection =
            serde_json::from_value(call.target).map_err(|_| refused())?;
        let projection: LeaseProjection =
            serde_json::from_value(call.request).map_err(|_| refused())?;
        let ObjectWork::Fleet { host, io, .. } = &self.work else {
            return Err(refused());
        };
        let (result, _) = StepDriver::new(host.clone())
            .run(StoreLeaseProjection {
                projection,
                now: chrono::DateTime::from_timestamp_millis(io.now().unix_millis())
                    .ok_or(CallFailure::AnswerLost)?,
                row_ids: crate::fleet_row_ids::FleetRowIds::new(io.clone()),
            })
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        serde_json::to_vec(&result).map_err(|_| CallFailure::AnswerLost)
    }
    pub(crate) async fn advance_lease_incarnation_call(
        &self,
        call: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        require_registry(&call)?;
        let _: AdvanceLeaseIncarnation =
            serde_json::from_value(call.target).map_err(|_| refused())?;
        let request: AdvanceLeaseIncarnationRequest =
            serde_json::from_value(call.request).map_err(|_| refused())?;
        let ObjectWork::Fleet { host, io, .. } = &self.work else {
            return Err(refused());
        };
        let mut ids = crate::fleet_row_ids::FleetRowIds::new(io.clone());
        let (incarnation, _) = StepDriver::new(host.clone())
            .run(AdvanceLeaseIncarnationStep {
                at_least: request.at_least,
                row_id: ids.next().map_err(|_| CallFailure::AnswerLost)?,
            })
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        serde_json::to_vec(&incarnation).map_err(|_| CallFailure::AnswerLost)
    }

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
    if call.source == ObjectAddress::Registry
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
