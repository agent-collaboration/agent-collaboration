//! Owner-only fixture control over the live Fleet store, absent from normal builds.

use crate::{
    AccessCaller, CallerContext, CallerKind, CentralApplication, CentralPlatform, EdgeKind,
    FleetKind, ObjectAddress, ObjectHost, ObjectRuntime, PlatformIo, StepConformance, StepControl,
    StepControlReply,
    caller_context::CallAuthority,
    object_call_envelope::{ObjectCallEnvelope, encode_shared_call},
    object_runtime::ObjectWork,
};
use axum::{
    Extension, Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use durable_steps::{AtomicStore, CallFailure, CallTarget, StoreError, storage::TransactionScope};
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::sync::Arc;

#[derive(Serialize, Deserialize)]
pub(crate) struct FleetReceiptProbe;
impl CallTarget for FleetReceiptProbe {
    const KIND: &'static str = "central.conformance-fleet-receipt";
    type Request = StepControl;
    type Answer = Result<StepControlReply, StoreError>;
}

pub(crate) async fn control<Platform: CentralPlatform>(
    State(application): State<Arc<CentralApplication<Platform>>>,
    Extension(caller): Extension<AccessCaller>,
    Json(input): Json<StepControl>,
) -> Response {
    if !matches!(caller, AccessCaller::Owner { .. }) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let result = async {
        let caller = CallerContext::<EdgeKind>::for_verified_edge(caller)
            .map_err(|_| CallFailure::AnswerLost)?;
        let identity = durable_steps::CallId {
            operation: durable_steps::OperationId::new(application.platform.ids().new_id())
                .map_err(|_| CallFailure::AnswerLost)?,
            step: durable_steps::StepName::new("fleet_fixture")
                .map_err(|_| CallFailure::AnswerLost)?,
        };
        let call = encode_shared_call::<EdgeKind, FleetKind, FleetReceiptProbe>(
            &caller,
            (),
            FleetReceiptProbe,
            input,
            identity,
        )?;
        let bytes = application.platform.transport_object(call).await?;
        serde_json::from_slice::<Result<StepControlReply, StoreError>>(&bytes)
            .map_err(|_| CallFailure::AnswerLost)
    }
    .await;
    crate::conformance_holder::probe_response(result)
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
    pub(crate) async fn receipt_probe(
        &self,
        call: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        if call.source != ObjectAddress::Registry
            || call.source_face != CallerKind::Edge
            || !matches!(call.authority, CallAuthority::Owner(_))
        {
            return Err(CallFailure::AnswerLost);
        }
        let _: FleetReceiptProbe =
            serde_json::from_value(call.target).map_err(|_| CallFailure::AnswerLost)?;
        let input: StepControl =
            serde_json::from_value(call.request).map_err(|_| CallFailure::AnswerLost)?;
        let ObjectWork::Fleet { host, io, .. } = &self.work else {
            return Err(CallFailure::AnswerLost);
        };
        let result = StepConformance::new(host.clone(), io.clone(), io.now())
            .execute(input)
            .await;
        serde_json::to_vec(&result).map_err(|_| CallFailure::AnswerLost)
    }
}
