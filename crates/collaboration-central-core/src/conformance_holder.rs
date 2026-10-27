//! A test-only task exercises the real Registry host and durable call journal.

use crate::{
    AccessCaller, CallerContext, CallerKind, CentralApplication, CentralPlatform, EdgeKind,
    ObjectAddress, ObjectHost, ObjectRuntime, PlatformIo, RegistryKind,
    caller_context::CallAuthority,
    object_call_envelope::{ObjectCallEnvelope, encode_shared_call},
    object_runtime::ObjectWork,
    step_driver::{StepCallback, StepDriver, StepResult},
};
use axum::{
    Extension, Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use durable_steps::{
    AtomicStore, BoxFuture, CallFailure, CallTarget, JournalScope, NotSentReason, OperationContext,
    OperationFailure, OperationId, OperationState, RequestKey, StoreError, Task,
    storage::{JournalStorage, StepRecord, TransactionScope},
};
use fleet_sync_lease::{HolderRelease, HolderWithdrawal};
use fleet_sync_protocol::{
    PeerId, ReleaseAnswer, ReleaseRequest, RequestId, WithdrawAnswer, WithdrawRelease,
};
use runtime_primitives::{Clock, SyncTime};
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::{convert::Infallible, marker::PhantomData, sync::Arc};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct HolderProbeInput {
    pub attempt: RequestId,
    pub holder: PeerId,
    pub request: HolderProbeRequest,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum HolderProbeRequest {
    Release(ReleaseRequest),
    Withdrawal(WithdrawRelease),
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum HolderProbeReply {
    Release(ReleaseAnswer),
    Withdrawal(WithdrawAnswer),
}
#[derive(Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum ProbeAnswer<Reply> {
    Completed(Reply),
    Pending {
        operation: OperationId,
        step: Option<StepRecord>,
    },
    Failed(OperationFailure),
}
type HolderProbeAnswer = ProbeAnswer<HolderProbeReply>;
#[derive(Serialize, Deserialize)]
pub(crate) struct ObserveRegistryDispatches;
impl CallTarget for ObserveRegistryDispatches {
    const KIND: &'static str = "central.conformance-registry-dispatches";
    type Request = ();
    type Answer = Vec<ObjectAddress>;
}
#[derive(Serialize, Deserialize)]
pub(crate) struct HolderProbe;
impl CallTarget for HolderProbe {
    const KIND: &'static str = "central.conformance-holder";
    type Request = HolderProbeInput;
    type Answer = HolderProbeAnswer;
}
#[derive(Serialize, Deserialize)]
pub(crate) struct HolderProbeStatus;
impl CallTarget for HolderProbeStatus {
    const KIND: &'static str = "central.conformance-holder-status";
    type Request = OperationId;
    type Answer = HolderProbeAnswer;
}
pub(crate) struct HolderProbeTask<Store>(pub PhantomData<fn() -> Store>);
impl<Store: AtomicStore> Task for HolderProbeTask<Store> {
    const KIND: &'static str = "central.conformance-holder-task";
    const VERSION: u32 = 1;
    type Store = Store;
    type Input = HolderProbeInput;
    type Output = HolderProbeReply;
    async fn run<Context: OperationContext<Store = Store>>(
        &self,
        context: &mut Context,
        input: &Self::Input,
    ) -> Result<Self::Output, OperationFailure> {
        let step = durable_steps::StepName::new("holder_request").map_err(|error| {
            OperationFailure::InvalidPayload {
                message: error.to_string(),
            }
        })?;
        Ok(match &input.request {
            HolderProbeRequest::Release(request) => HolderProbeReply::Release(
                context
                    .call(
                        step,
                        HolderRelease {
                            holder: input.holder,
                            database: request.database,
                        },
                        request.clone(),
                    )
                    .await?,
            ),
            HolderProbeRequest::Withdrawal(request) => HolderProbeReply::Withdrawal(
                context
                    .call(
                        step,
                        HolderWithdrawal {
                            holder: input.holder,
                        },
                        request.clone(),
                    )
                    .await?,
            ),
        })
    }
}

pub(crate) async fn control<Platform: CentralPlatform>(
    State(application): State<Arc<CentralApplication<Platform>>>,
    Extension(caller): Extension<AccessCaller>,
    Json(input): Json<HolderProbeInput>,
) -> Response {
    probe_response(invoke_probe(&application, caller, HolderProbe, input).await)
}

pub(crate) async fn status<Platform: CentralPlatform>(
    State(application): State<Arc<CentralApplication<Platform>>>,
    Extension(caller): Extension<AccessCaller>,
    Json(operation): Json<OperationId>,
) -> Response {
    probe_response(invoke_probe(&application, caller, HolderProbeStatus, operation).await)
}

pub(crate) async fn dispatches<Platform: CentralPlatform>(
    State(application): State<Arc<CentralApplication<Platform>>>,
    Extension(caller): Extension<AccessCaller>,
) -> Response {
    probe_response(invoke_probe(&application, caller, ObserveRegistryDispatches, ()).await)
}

pub(crate) async fn invoke_probe<Platform: CentralPlatform, Call: CallTarget>(
    application: &CentralApplication<Platform>,
    caller: AccessCaller,
    target: Call,
    input: Call::Request,
) -> Result<Call::Answer, CallFailure> {
    let caller = CallerContext::<EdgeKind>::for_verified_edge(caller).map_err(|_| refused())?;
    let identity = durable_steps::CallId {
        operation: OperationId::new(application.platform.ids().new_id()).map_err(|_| refused())?,
        step: durable_steps::StepName::new("holder_probe").map_err(|_| refused())?,
    };
    let call =
        encode_shared_call::<EdgeKind, RegistryKind, Call>(&caller, (), target, input, identity)?;
    let answer = application.platform.transport_object(call).await?;
    serde_json::from_slice(&answer).map_err(|_| CallFailure::AnswerLost)
}

pub(crate) fn probe_response<Answer: Serialize>(result: Result<Answer, CallFailure>) -> Response {
    match result {
        Ok(answer) => Json(answer).into_response(),
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<RegistryKind, Host, Io>
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
    pub(crate) async fn holder_probe(
        &self,
        invocation: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        if invocation.source_face != CallerKind::Edge
            || invocation.source != ObjectAddress::Registry
            || !matches!(invocation.authority, CallAuthority::Owner(_))
        {
            return Err(refused());
        }
        let _: HolderProbe = serde_json::from_value(invocation.target).map_err(|_| refused())?;
        let input: HolderProbeInput =
            serde_json::from_value(invocation.request).map_err(|_| refused())?;
        let ObjectWork::Registry {
            host,
            engine,
            shared_host,
            ..
        } = &self.work
        else {
            return Err(refused());
        };
        let (operation, _) = StepDriver::new(host.clone())
            .run(AdmitProbe {
                input,
                now: shared_host.now(),
            })
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        engine
            .drive_due()
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        self.read_probe_status::<HolderProbeReply>(operation).await
    }

    pub(crate) fn registry_dispatches(
        &self,
        invocation: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        if invocation.source_face != CallerKind::Edge
            || invocation.source != ObjectAddress::Registry
            || !matches!(invocation.authority, CallAuthority::Owner(_))
        {
            return Err(refused());
        }
        let _: ObserveRegistryDispatches =
            serde_json::from_value(invocation.target).map_err(|_| refused())?;
        let _: () = serde_json::from_value(invocation.request).map_err(|_| refused())?;
        let ObjectWork::Registry { shared_host, .. } = &self.work else {
            return Err(refused());
        };
        let destinations = shared_host
            .dispatched_destinations()
            .map_err(|_| CallFailure::AnswerLost)?;
        serde_json::to_vec(&destinations).map_err(|_| CallFailure::AnswerLost)
    }

    pub(crate) async fn holder_probe_status(
        &self,
        invocation: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        if invocation.source_face != CallerKind::Edge
            || invocation.source != ObjectAddress::Registry
            || !matches!(invocation.authority, CallAuthority::Owner(_))
        {
            return Err(refused());
        }
        let _: HolderProbeStatus =
            serde_json::from_value(invocation.target).map_err(|_| refused())?;
        let operation = serde_json::from_value(invocation.request).map_err(|_| refused())?;
        self.read_probe_status::<HolderProbeReply>(operation).await
    }

    pub(crate) async fn read_probe_status<Reply: Serialize + serde::de::DeserializeOwned>(
        &self,
        operation: OperationId,
    ) -> Result<Vec<u8>, CallFailure> {
        let ObjectWork::Registry { host, engine, .. } = &self.work else {
            return Err(refused());
        };
        let answer = match engine
            .status(&operation)
            .await
            .map_err(|_| CallFailure::AnswerLost)?
            .state
        {
            OperationState::Completed { result } => ProbeAnswer::<Reply>::Completed(
                serde_json::from_slice(&result).map_err(|_| CallFailure::AnswerLost)?,
            ),
            OperationState::Failed { reason } => ProbeAnswer::Failed(reason),
            OperationState::Running { .. } => {
                let (step, _) = StepDriver::new(host.clone())
                    .run(ReadProbeStep { operation })
                    .await
                    .map_err(|_| CallFailure::AnswerLost)?;
                ProbeAnswer::Pending { operation, step }
            }
        };
        serde_json::to_vec(&answer).map_err(|_| CallFailure::AnswerLost)
    }
}
struct AdmitProbe {
    input: HolderProbeInput,
    now: SyncTime,
}
struct ReadProbeStep {
    operation: OperationId,
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for AdmitProbe {
    type Output = OperationId;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let key =
                RequestKey::new(self.input.attempt.to_string()).map_err(StoreError::backend)?;
            let admitted = scope
                .admit::<HolderProbeTask<Store>>(&key, &self.input)
                .await
                .map_err(StoreError::backend)?;
            scope.ensure_wake(self.now).await?;
            Ok((admitted.id(), Vec::new()))
        })
    }
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for ReadProbeStep {
    type Output = Option<StepRecord>;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move { Ok((scope.step(self.operation, 0).await?, Vec::new())) })
    }
}
fn refused() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}
