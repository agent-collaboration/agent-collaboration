//! Canonical Fleet calls keep ServiceId distinct from the legacy installation type.

use crate::{
    CallerKind, FleetCall, FleetKind, FleetResult, ObjectAddress, ObjectHost, ObjectRuntime,
    PlatformIo, WireCall,
    caller_context::CallAuthority,
    fleet_metadata_step::{DecideFleet, ImportFleet},
    object_call_envelope::ObjectCallEnvelope,
    object_runtime::ObjectWork,
    step_driver::StepDriver,
};
use durable_steps::{
    AtomicStore, CallFailure, CallTarget, NotSentReason, storage::TransactionScope,
};
use fleet_admission::{FleetRequest, FleetRequestResult};
use message_board::ActorOrigin;
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

#[derive(Serialize, Deserialize)]
pub(crate) struct FleetMetadata;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct FleetMetadataRequest {
    pub request: FleetRequest,
    pub origin: ActorOrigin,
}
impl FleetMetadataRequest {
    pub(crate) fn from_envelope(
        envelope: board_sync_protocol::FleetRequestEnvelope,
        origin: ActorOrigin,
    ) -> Option<Self> {
        let request: FleetRequest = envelope.operation.decode().ok()?;
        let value = Self { request, origin };
        (value.request.request_id == envelope.request_id && value.actor_matches_origin())
            .then_some(value)
    }

    fn actor_matches_origin(&self) -> bool {
        !matches!(&self.request.actor, message_board::RecordedActor::Router { installation, .. }
            if installation != &self.origin.installation)
    }
}
impl CallTarget for FleetMetadata {
    const KIND: &'static str = "central.fleet-metadata";
    type Request = FleetMetadataRequest;
    type Answer = FleetRequestResult;
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
    pub(crate) async fn import_call(
        &self,
        invocation: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        if !invocation.target.is_null()
            || invocation.source_face != CallerKind::Edge
            || !matches!(invocation.authority, CallAuthority::Owner(_))
        {
            return Err(refused());
        }
        let WireCall::Fleet(FleetCall::Import { bundle }) =
            serde_json::from_value(invocation.request).map_err(|_| refused())?
        else {
            return Err(refused());
        };
        let ObjectWork::Fleet { host, io, .. } = &self.work else {
            return Err(refused());
        };
        let now = chrono::DateTime::from_timestamp_millis(io.now().unix_millis())
            .ok_or(CallFailure::AnswerLost)?;
        let row_ids = crate::fleet_row_ids::FleetRowIds::new(io.clone());
        let (result, _) = StepDriver::new(host.clone())
            .run(ImportFleet {
                bundle: *bundle,
                now,
                row_ids,
            })
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        serde_json::to_vec(&FleetResult::Imported(result)).map_err(|_| CallFailure::AnswerLost)
    }

    pub(crate) async fn metadata_call(
        &self,
        invocation: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        if invocation.source_face != CallerKind::Session
            || !matches!(invocation.authority, CallAuthority::Object)
        {
            return Err(refused());
        }
        let _: FleetMetadata = serde_json::from_value(invocation.target).map_err(|_| refused())?;
        let bound: FleetMetadataRequest =
            serde_json::from_value(invocation.request).map_err(|_| refused())?;
        if invocation.source != ObjectAddress::Connection(bound.origin.installation.clone())
            || !bound.actor_matches_origin()
        {
            return Err(refused());
        }
        let FleetMetadataRequest { request, origin } = bound;
        let ObjectWork::Fleet { host, io, .. } = &self.work else {
            return Err(refused());
        };
        let now = chrono::DateTime::from_timestamp_millis(io.now().unix_millis())
            .ok_or(CallFailure::AnswerLost)?;
        let row_ids = crate::fleet_row_ids::FleetRowIds::new(io.clone());
        let (result, _) = StepDriver::new(host.clone())
            .run(DecideFleet {
                request,
                origin,
                now,
                row_ids,
            })
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        serde_json::to_vec(&result).map_err(|_| CallFailure::AnswerLost)
    }
}
fn refused() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}
