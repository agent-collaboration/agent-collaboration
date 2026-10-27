//! Hold and replay real enrollment lifecycle calls; observe the owning row and journal.

use super::{
    AtomicStore, BoxFuture, ColumnIndex, Database, Decode, Deserialize, Encode, EnrollmentState,
    Infallible, IntoArguments, ObjectHost, ObjectRuntime, ObjectWork, PeerId, PlatformIo,
    RegistryKind, Serialize, ServiceId, StepCallback, StepDriver, StepResult, StoreError,
    StoredMachine, TransactionScope, Type, corrupt, queries,
};
use crate::{
    AccessCaller, CallerKind, CentralApplication, CentralPlatform, ObjectAddress,
    caller_context::CallAuthority, object_call_envelope::ObjectCallEnvelope,
};
use axum::{Extension, Json, extract::State, response::Response};
use durable_steps::{
    CallFailure, CallId, CallTarget, OperationId, OperationStatus, storage::JournalStorage,
};
use futures_channel::oneshot;
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};

mod lifecycle_pause;
pub use lifecycle_pause::LifecyclePauseObservation;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum RevocationControl {
    Observe,
    ArmDelivery,
    ReleaseDelivery,
    ArmActivation,
    ReleaseActivation,
    ArmOpen,
    ReleaseOpen,
    ArmBeforeActivation,
    ReleaseBeforeActivation,
    Replay { call: CallId },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevocationControlInput {
    pub service: ServiceId,
    pub control: RevocationControl,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevocationEnrollmentSnapshot {
    pub peer: PeerId,
    pub last_revocation_operation: Option<OperationId>,
    pub last_enrollment_operation: Option<OperationId>,
    pub revoked: bool,
    pub fingerprint: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevocationObservation {
    pub gate_armed: bool,
    pub gate_reached: bool,
    pub calls: Vec<CallId>,
    pub enrollment: Option<RevocationEnrollmentSnapshot>,
    pub operation: Option<OperationStatus>,
    pub activation_pause: LifecyclePauseObservation,
    pub open_pause: LifecyclePauseObservation,
    pub before_activation_pause: LifecyclePauseObservation,
    pub open_calls: Vec<CallId>,
    pub close_calls: Vec<CallId>,
    pub readmission_operation: Option<OperationStatus>,
    pub activation_step: Option<durable_steps::storage::StepRecord>,
    pub open_step: Option<durable_steps::storage::StepRecord>,
}

#[derive(Clone)]
pub(crate) struct RecordedLifecycleCall {
    pub identity: CallId,
    pub destination: ObjectAddress,
    pub bytes: Vec<u8>,
}
#[derive(Default)]
struct ProbeState {
    armed: bool,
    reached: bool,
    calls: Vec<RecordedLifecycleCall>,
    open_calls: Vec<RecordedLifecycleCall>,
    close_calls: Vec<RecordedLifecycleCall>,
    release: Vec<oneshot::Sender<()>>,
}
#[derive(Default)]
pub(crate) struct RevocationProbe {
    state: Mutex<ProbeState>,
    pub(crate) activation_pause: lifecycle_pause::LifecyclePause,
    pub(crate) open_pause: lifecycle_pause::LifecyclePause,
    pub(crate) before_activation_pause: lifecycle_pause::LifecyclePause,
}
impl RevocationProbe {
    pub(crate) async fn before_dispatch(
        &self,
        call: RecordedLifecycleCall,
    ) -> Result<(), StoreError> {
        let pending = {
            let mut state = self.state.lock().map_err(StoreError::backend)?;
            state.calls.push(call);
            if state.armed {
                state.reached = true;
                let (release, pending) = oneshot::channel();
                state.release.push(release);
                Some(pending)
            } else {
                None
            }
        };
        if let Some(pending) = pending {
            pending.await.map_err(StoreError::backend)?;
        }
        Ok(())
    }
    pub(crate) async fn before_open_dispatch(
        &self,
        call: RecordedLifecycleCall,
    ) -> Result<(), StoreError> {
        self.state
            .lock()
            .map_err(StoreError::backend)?
            .open_calls
            .push(call);
        self.open_pause.pause().await
    }
    pub(crate) fn record_close_dispatch(
        &self,
        call: RecordedLifecycleCall,
    ) -> Result<(), StoreError> {
        self.state
            .lock()
            .map_err(StoreError::backend)?
            .close_calls
            .push(call);
        Ok(())
    }
    fn apply(&self, control: &RevocationControl) -> Result<(), StoreError> {
        match control {
            RevocationControl::ArmActivation => return self.activation_pause.arm(),
            RevocationControl::ReleaseActivation => return self.activation_pause.release(),
            RevocationControl::ArmOpen => return self.open_pause.arm(),
            RevocationControl::ReleaseOpen => return self.open_pause.release(),
            RevocationControl::ArmBeforeActivation => return self.before_activation_pause.arm(),
            RevocationControl::ReleaseBeforeActivation => {
                return self.before_activation_pause.release();
            }
            _ => {}
        }
        let releases = {
            let mut state = self.state.lock().map_err(StoreError::backend)?;
            match control {
                RevocationControl::ArmDelivery => {
                    if state.armed {
                        return Err(StoreError::Aborted {
                            reason: "revocation delivery already armed".into(),
                        });
                    }
                    state.armed = true;
                    state.reached = false;
                    Vec::new()
                }
                RevocationControl::ReleaseDelivery => {
                    state.armed = false;
                    std::mem::take(&mut state.release)
                }
                RevocationControl::Observe
                | RevocationControl::Replay { .. }
                | RevocationControl::ArmActivation
                | RevocationControl::ReleaseActivation
                | RevocationControl::ArmOpen
                | RevocationControl::ReleaseOpen
                | RevocationControl::ArmBeforeActivation
                | RevocationControl::ReleaseBeforeActivation => Vec::new(),
            }
        };
        for release in releases {
            let _ = release.send(());
        }
        Ok(())
    }
    pub(crate) fn recorded(&self, identity: &CallId) -> Result<RecordedLifecycleCall, StoreError> {
        let state = self.state.lock().map_err(StoreError::backend)?;
        state
            .calls
            .iter()
            .chain(&state.open_calls)
            .chain(&state.close_calls)
            .find(|call| &call.identity == identity)
            .cloned()
            .ok_or_else(|| StoreError::Aborted {
                reason: "lifecycle call was not recorded".into(),
            })
    }
    fn observation(&self) -> Result<(bool, bool, Vec<CallId>), StoreError> {
        let state = self.state.lock().map_err(StoreError::backend)?;
        Ok((
            state.armed,
            state.reached,
            state
                .calls
                .iter()
                .map(|call| call.identity.clone())
                .collect(),
        ))
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct ControlRevocation;
impl CallTarget for ControlRevocation {
    const KIND: &'static str = "central.conformance-revocation";
    type Request = RevocationControlInput;
    type Answer = RevocationObservation;
}
pub(crate) async fn control<Platform: CentralPlatform>(
    State(application): State<Arc<CentralApplication<Platform>>>,
    Extension(caller): Extension<AccessCaller>,
    Json(input): Json<RevocationControlInput>,
) -> Response {
    crate::conformance_holder::probe_response(
        crate::conformance_holder::invoke_probe(&application, caller, ControlRevocation, input)
            .await,
    )
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
    pub(crate) async fn control_revocation(
        &self,
        invocation: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        if invocation.source_face != CallerKind::Edge
            || invocation.source != ObjectAddress::Registry
            || !matches!(invocation.authority, CallAuthority::Owner(_))
        {
            return Err(CallFailure::AnswerLost);
        }
        let _: ControlRevocation =
            serde_json::from_value(invocation.target).map_err(|_| CallFailure::AnswerLost)?;
        let input: RevocationControlInput =
            serde_json::from_value(invocation.request).map_err(|_| CallFailure::AnswerLost)?;
        let ObjectWork::Registry {
            host, shared_host, ..
        } = &self.work
        else {
            return Err(CallFailure::AnswerLost);
        };
        shared_host
            .revocation_probe
            .apply(&input.control)
            .map_err(|_| CallFailure::AnswerLost)?;
        if let RevocationControl::Replay { call } = input.control {
            shared_host.replay_lifecycle_call(&call).await?;
        }
        let ((enrollment, operation, readmission_operation, activation_step, open_step), _) =
            StepDriver::new(host.clone())
                .run(ReadRevocation {
                    service: input.service,
                })
                .await
                .map_err(|_| CallFailure::AnswerLost)?;
        let (gate_armed, gate_reached, calls) = shared_host
            .revocation_probe
            .observation()
            .map_err(|_| CallFailure::AnswerLost)?;
        let (open_calls, close_calls) = {
            let state = shared_host
                .revocation_probe
                .state
                .lock()
                .map_err(|_| CallFailure::AnswerLost)?;
            (
                state
                    .open_calls
                    .iter()
                    .map(|call| call.identity.clone())
                    .collect(),
                state
                    .close_calls
                    .iter()
                    .map(|call| call.identity.clone())
                    .collect(),
            )
        };
        serde_json::to_vec(&RevocationObservation {
            gate_armed,
            gate_reached,
            calls,
            enrollment,
            operation,
            activation_pause: shared_host
                .revocation_probe
                .activation_pause
                .observation()
                .map_err(|_| CallFailure::AnswerLost)?,
            open_pause: shared_host
                .revocation_probe
                .open_pause
                .observation()
                .map_err(|_| CallFailure::AnswerLost)?,
            before_activation_pause: shared_host
                .revocation_probe
                .before_activation_pause
                .observation()
                .map_err(|_| CallFailure::AnswerLost)?,
            open_calls,
            close_calls,
            readmission_operation,
            activation_step,
            open_step,
        })
        .map_err(|_| CallFailure::AnswerLost)
    }
}

struct ReadRevocation {
    service: ServiceId,
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for ReadRevocation
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = (
        Option<RevocationEnrollmentSnapshot>,
        Option<OperationStatus>,
        Option<OperationStatus>,
        Option<durable_steps::storage::StepRecord>,
        Option<durable_steps::storage::StepRecord>,
    );
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let mut rows = queries::read_enrollments(
                scope,
                Some(self.service.as_str().into()),
                None,
                None,
                None,
            )
            .await
            .map_err(StoreError::backend)?;
            if rows.len() > 1 {
                return Err(corrupt("service_id"));
            }
            let Some(machine) = rows.pop().map(StoredMachine::decode).transpose()? else {
                return Ok(((None, None, None, None, None), Vec::new()));
            };
            let fingerprint = format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&machine).map_err(StoreError::backend)?)
            );
            let operation = match machine.last_revocation_operation {
                Some(id) => Some(
                    scope
                        .operation(id)
                        .await?
                        .ok_or(StoreError::MissingOperation { id })?
                        .status(),
                ),
                None => None,
            };
            let (readmission_operation, activation_step, open_step) =
                match machine.last_enrollment_operation {
                    Some(id) => (
                        Some(
                            scope
                                .operation(id)
                                .await?
                                .ok_or(StoreError::MissingOperation { id })?
                                .status(),
                        ),
                        scope.step(id, 1).await?,
                        scope.step(id, 0).await?,
                    ),
                    None => (None, None, None),
                };
            Ok((
                (
                    Some(RevocationEnrollmentSnapshot {
                        peer: machine.peer,
                        last_revocation_operation: machine.last_revocation_operation,
                        last_enrollment_operation: machine.last_enrollment_operation,
                        revoked: machine.state == EnrollmentState::Revoked,
                        fingerprint,
                    }),
                    operation,
                    readmission_operation,
                    activation_step,
                    open_step,
                ),
                Vec::new(),
            ))
        })
    }
}
