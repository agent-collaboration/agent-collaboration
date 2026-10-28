//! Copy proof uses the real Registry caller, Copy object and durable step journal.

use crate::{
    AccessCaller, CallerKind, CentralApplication, CentralPlatform, ObjectAddress, ObjectHost,
    ObjectRuntime, PlatformIo, RegistryKind,
    caller_context::CallAuthority,
    conformance_copy_wait::{ControlCopyWait, CopyWaitInput, CopyWaitTarget},
    conformance_holder::{ProbeAnswer, invoke_probe, probe_response},
    object_call_envelope::ObjectCallEnvelope,
    object_runtime::ObjectWork,
    step_driver::{StepCallback, StepDriver, StepResult},
};
use axum::{Extension, Json, extract::State, response::Response};
use durable_steps::{
    AtomicStore, BoxFuture, CallFailure, CallTarget, Host as _, JournalScope, NotSentReason,
    OperationContext, OperationFailure, OperationId, RequestKey, StoreError, Task,
    storage::TransactionScope,
};
use fleet_sync_lease::{
    CopyAcknowledged, CopyCreate, CopyFence, CopyRead, CopyRetire, HolderRelease,
};
use fleet_sync_protocol::{
    AwaitAcked, AwaitAckedAnswer, CopyStatusAnswer, CreateCopy, CreateCopyAnswer, DatabaseId,
    FenceRequest, FenceResult, PeerId, ReceiverId, ReleaseAnswer, ReleaseRequest, RequestId,
    RetireCopy, RetireCopyAnswer,
};
use runtime_primitives::{Clock, SyncTime};
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::{convert::Infallible, marker::PhantomData, sync::Arc};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CopyProbeInput {
    attempt: RequestId,
    copy: ReceiverId,
    request: CopyProbeRequest,
}
#[derive(Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
enum CopyProbeRequest {
    Create(CreateCopy),
    Fence(FenceRequest),
    Retire(RetireCopy),
    Read(DatabaseId),
    Acknowledged {
        database: DatabaseId,
        request: AwaitAcked,
    },
    ReleaseThenAcknowledged {
        holder: PeerId,
        request: ReleaseRequest,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum CopyProbeReply {
    Created(CreateCopyAnswer),
    Fenced(FenceResult),
    Retired(RetireCopyAnswer),
    Read(CopyStatusAnswer),
    Acknowledged(AwaitAckedAnswer),
}

#[derive(Serialize, Deserialize)]
pub(crate) struct CopyProbe;
impl CallTarget for CopyProbe {
    const KIND: &'static str = "central.conformance-copy";
    type Request = CopyProbeInput;
    type Answer = ProbeAnswer<CopyProbeReply>;
}
#[derive(Serialize, Deserialize)]
pub(crate) struct CopyProbeStatus;
impl CallTarget for CopyProbeStatus {
    const KIND: &'static str = "central.conformance-copy-status";
    type Request = OperationId;
    type Answer = ProbeAnswer<CopyProbeReply>;
}

#[derive(Serialize, Deserialize)]
pub(crate) struct ObserveDriveTokens;
impl CallTarget for ObserveDriveTokens {
    const KIND: &'static str = "central.conformance-drive-tokens";
    type Request = ();
    type Answer = Vec<OperationId>;
}

pub(crate) struct CopyProbeTask<Store>(pub PhantomData<fn() -> Store>);
impl<Store: AtomicStore> Task for CopyProbeTask<Store> {
    const KIND: &'static str = "central.conformance-copy-task";
    const VERSION: u32 = 1;
    type Store = Store;
    type Input = CopyProbeInput;
    type Output = CopyProbeReply;
    async fn run<Context: OperationContext<Store = Store>>(
        &self,
        context: &mut Context,
        input: &Self::Input,
    ) -> Result<Self::Output, OperationFailure> {
        let step = durable_steps::StepName::new("copy_request").map_err(|error| {
            OperationFailure::InvalidPayload {
                message: error.to_string(),
            }
        })?;
        Ok(match &input.request {
            CopyProbeRequest::Create(request) => CopyProbeReply::Created(
                context
                    .call(step, CopyCreate { copy: input.copy }, request.clone())
                    .await?,
            ),
            CopyProbeRequest::Fence(request) => CopyProbeReply::Fenced(
                context
                    .call(step, CopyFence { copy: input.copy }, request.clone())
                    .await?,
            ),
            CopyProbeRequest::Retire(request) => CopyProbeReply::Retired(
                context
                    .call(step, CopyRetire { copy: input.copy }, request.clone())
                    .await?,
            ),
            CopyProbeRequest::Read(database) => CopyProbeReply::Read(
                context
                    .call(
                        step,
                        CopyRead {
                            copy: input.copy,
                            database: *database,
                        },
                        (),
                    )
                    .await?,
            ),
            CopyProbeRequest::Acknowledged { database, request } => CopyProbeReply::Acknowledged(
                context
                    .call(
                        step,
                        CopyAcknowledged {
                            copy: input.copy,
                            database: *database,
                        },
                        request.clone(),
                    )
                    .await?,
            ),
            CopyProbeRequest::ReleaseThenAcknowledged { holder, request } => {
                let release_step =
                    durable_steps::StepName::new("release_first").map_err(|error| {
                        OperationFailure::InvalidPayload {
                            message: error.to_string(),
                        }
                    })?;
                let answer = context
                    .call(
                        release_step,
                        HolderRelease {
                            holder: *holder,
                            database: request.database,
                        },
                        request.clone(),
                    )
                    .await?;
                let ReleaseAnswer::Released { final_position } = answer else {
                    return Err(OperationFailure::Refused {
                        code: "conformance_release_refused".into(),
                        detail: format!("{answer:?}"),
                    });
                };
                CopyProbeReply::Acknowledged(
                    context
                        .call(
                            step,
                            CopyAcknowledged {
                                copy: input.copy,
                                database: request.database,
                            },
                            AwaitAcked {
                                through: final_position,
                            },
                        )
                        .await?,
                )
            }
        })
    }
}

pub(crate) async fn control<Platform: CentralPlatform>(
    State(application): State<Arc<CentralApplication<Platform>>>,
    Extension(caller): Extension<AccessCaller>,
    Json(input): Json<CopyProbeInput>,
) -> Response {
    probe_response(invoke_probe(&application, caller, CopyProbe, input).await)
}
pub(crate) async fn status<Platform: CentralPlatform>(
    State(application): State<Arc<CentralApplication<Platform>>>,
    Extension(caller): Extension<AccessCaller>,
    Json(operation): Json<OperationId>,
) -> Response {
    probe_response(invoke_probe(&application, caller, CopyProbeStatus, operation).await)
}

pub(crate) async fn wait_control<Platform: CentralPlatform>(
    State(application): State<Arc<CentralApplication<Platform>>>,
    Extension(caller): Extension<AccessCaller>,
    Json(input): Json<CopyWaitInput>,
) -> Response {
    probe_response(invoke_probe(&application, caller, ControlCopyWait, input).await)
}

pub(crate) async fn drive_tokens<Platform: CentralPlatform>(
    State(application): State<Arc<CentralApplication<Platform>>>,
    Extension(caller): Extension<AccessCaller>,
) -> Response {
    probe_response(invoke_probe(&application, caller, ObserveDriveTokens, ()).await)
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
    pub(crate) async fn copy_probe(
        &self,
        invocation: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        check_owner(&invocation)?;
        let _: CopyProbe = serde_json::from_value(invocation.target).map_err(|_| refused())?;
        let input = serde_json::from_value(invocation.request).map_err(|_| refused())?;
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
            .run(AdmitCopyProbe {
                input,
                now: shared_host.now(),
            })
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        engine
            .drive_due()
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        self.read_probe_status::<CopyProbeReply>(operation).await
    }

    pub(crate) async fn copy_probe_status(
        &self,
        invocation: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        check_owner(&invocation)?;
        let _: CopyProbeStatus =
            serde_json::from_value(invocation.target).map_err(|_| refused())?;
        let operation = serde_json::from_value(invocation.request).map_err(|_| refused())?;
        self.read_probe_status::<CopyProbeReply>(operation).await
    }

    pub(crate) async fn control_copy_wait(
        &self,
        invocation: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        check_owner(&invocation)?;
        let _: ControlCopyWait =
            serde_json::from_value(invocation.target).map_err(|_| refused())?;
        let input: CopyWaitInput =
            serde_json::from_value(invocation.request).map_err(|_| refused())?;
        let ObjectWork::Registry { shared_host, .. } = &self.work else {
            return Err(refused());
        };
        // The observer neither admits nor drives journal operations. The control
        // and read occur at the normal owning Copy object on either runtime.
        let observation = shared_host
            .dispatch(
                CopyWaitTarget {
                    copy: input.copy,
                    database: input.database,
                },
                input.control,
                invocation.identity,
            )
            .await?;
        serde_json::to_vec(&observation).map_err(|_| CallFailure::AnswerLost)
    }

    pub(crate) fn observe_drive_tokens(
        &self,
        invocation: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        check_owner(&invocation)?;
        let _: ObserveDriveTokens =
            serde_json::from_value(invocation.target).map_err(|_| refused())?;
        let _: () = serde_json::from_value(invocation.request).map_err(|_| refused())?;
        let ObjectWork::Registry { host, .. } = &self.work else {
            return Err(refused());
        };
        let rejected = host
            .stale_drive_rejections()
            .map_err(|_| CallFailure::AnswerLost)?;
        serde_json::to_vec(&rejected).map_err(|_| CallFailure::AnswerLost)
    }
}

struct AdmitCopyProbe {
    input: CopyProbeInput,
    now: SyncTime,
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for AdmitCopyProbe {
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
                .admit::<CopyProbeTask<Store>>(&key, &self.input)
                .await
                .map_err(StoreError::backend)?;
            scope.ensure_wake(self.now).await?;
            Ok((admitted.id(), Vec::new()))
        })
    }
}
fn check_owner(invocation: &ObjectCallEnvelope) -> Result<(), CallFailure> {
    if invocation.source_face == CallerKind::Edge
        && invocation.source == ObjectAddress::Registry
        && matches!(invocation.authority, CallAuthority::Owner(_))
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
