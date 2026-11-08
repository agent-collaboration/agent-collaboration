//! The registry owns the stable mapping from a Router identity to a library peer.
use crate::FleetKind;

use crate::machine_enrollment_record::{EnrollmentState, StoredMachine, corrupt, valid_hash};
use crate::{
    AccessClientId, AdmissionOrigin, ObjectHost, ObjectRuntime, OwnerIdentity, PlatformIo,
    generated_enrollment as queries,
    object_runtime::ObjectWork,
    step_driver::{StepCallback, StepDriver, StepResult},
};
use chrono::{DateTime, Utc};
use durable_steps::{AtomicStore, BoxFuture, StoreError, storage::TransactionScope};
use durable_steps::{
    JournalScope, OperationId, RequestKey,
    storage::{JournalStorage, OperationExecution},
};
use fleet_sync_protocol::{AuthRefusal, ConnectionId, PeerId, ReceiverId};
use fleet_sync_session::{ConnectionBinding, VerifiedPeer};
use message_board::{MachineLabel, ServiceId};
use runtime_primitives::{Clock, IdSource, SyncTime};
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;
pub(crate) mod admission;
mod readmission;
mod revocation;
pub(crate) use readmission::ReadmissionTask;
pub(crate) use revocation::{EndPeerSessions, RevokeMachineTask};
#[cfg(feature = "conformance")]
pub(crate) mod conformance_revocation;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnrollMachine {
    pub installation: ServiceId,
    pub peer: PeerId,
    pub label: MachineLabel,
    pub access_client_id: AccessClientId,
    pub credential_hash: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum EnrollmentDecision {
    MachineEnrolled {
        peer: PeerId,
        expires_at: DateTime<Utc>,
        authoritative_receiver: ReceiverId,
        relay_peer: PeerId,
    },
    Refused(EnrollmentRefusal),
    /// The caller may retry once the earlier revoke operation has completed.
    RevocationInProgress,
    /// Reserved identities become active after the enrollment workflow finishes.
    EnrollmentInProgress,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum RevocationDecision {
    MachineRevoked,
    EnrollmentInProgress,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EnrollmentRefusal {
    PeerInUse,
    PeerMismatch,
    AccessClientInUse,
    CredentialHashInUse,
    InvalidInput,
}

/// Registry authentication result with the admitted provenance snapshot.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdmittedMachine {
    peer: PeerId,
    origin: AdmissionOrigin,
    expires_at: SyncTime,
}
impl AdmittedMachine {
    pub fn peer(&self) -> PeerId {
        self.peer
    }
    pub fn origin(&self) -> &AdmissionOrigin {
        &self.origin
    }
    pub fn binding(&self, connection: ConnectionId) -> ConnectionBinding {
        ConnectionBinding::new(
            connection,
            VerifiedPeer {
                peer: self.peer,
                expires_at: self.expires_at,
            },
        )
    }
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
    pub async fn enroll_machine(
        &self,
        _owner: &OwnerIdentity,
        request: EnrollMachine,
    ) -> Result<EnrollmentDecision, StoreError> {
        let ObjectWork::Fleet {
            host,
            shared_host,
            engine,
            delivery,
            organization_stamp,
            ..
        } = &self.work
        else {
            return Err(wrong_face());
        };
        if organization_stamp.is_sign_in_service(&request.installation) {
            return Ok(EnrollmentDecision::Refused(EnrollmentRefusal::InvalidInput));
        }
        let now = DateTime::from_timestamp_millis(shared_host.now().unix_millis())
            .ok_or_else(|| corrupt("clock"))?;
        let (admission, _) = StepDriver::new(host.clone())
            .run(EnrollStep {
                request,
                now,
                authoritative_receiver: shared_host.authoritative_receiver(),
                relay_peer: delivery.relay_peer,
                nonce: shared_host.new_id(),
            })
            .await?;
        match admission {
            EnrollmentAdmission::Answered(decision) => Ok(decision),
            EnrollmentAdmission::Readmitting {
                operation,
                peer,
                expires_at,
                authoritative_receiver,
                relay_peer,
            } => {
                engine.drive_due().await?;
                if matches!(
                    engine.status(&operation).await?.state,
                    durable_steps::OperationState::Completed { .. }
                ) {
                    Ok(EnrollmentDecision::MachineEnrolled {
                        peer,
                        expires_at,
                        authoritative_receiver,
                        relay_peer,
                    })
                } else {
                    Ok(EnrollmentDecision::EnrollmentInProgress)
                }
            }
        }
    }

    pub async fn authenticate_machine(
        &self,
        access_client_id: AccessClientId,
        credential_hash: String,
    ) -> Result<Result<AdmittedMachine, AuthRefusal>, StoreError> {
        let ObjectWork::Fleet {
            host, shared_host, ..
        } = &self.work
        else {
            return Err(wrong_face());
        };
        let now = shared_host.now();
        let (decision, _) = StepDriver::new(host.clone())
            .run(AuthenticateStep {
                access_client_id,
                credential_hash,
                now,
            })
            .await?;
        Ok(decision)
    }
}

struct EnrollStep {
    request: EnrollMachine,
    now: DateTime<Utc>,
    authoritative_receiver: ReceiverId,
    relay_peer: PeerId,
    nonce: durable_steps::Uuid,
}
enum EnrollmentAdmission {
    Answered(EnrollmentDecision),
    Readmitting {
        operation: OperationId,
        peer: PeerId,
        expires_at: DateTime<Utc>,
        authoritative_receiver: ReceiverId,
        relay_peer: PeerId,
    },
}
impl<Store, Driver> StepCallback<Store, Driver> for EnrollStep
where
    Store: AtomicStore,
    Driver: Database,
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = EnrollmentAdmission;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let request = self.request;
            if request.expires_at <= self.now || !valid_hash(&request.credential_hash) {
                return Ok((
                    EnrollmentAdmission::Answered(EnrollmentDecision::Refused(
                        EnrollmentRefusal::InvalidInput,
                    )),
                    Vec::new(),
                ));
            }
            let found = queries::read_enrollments(
                scope,
                Some(request.installation.as_str().into()),
                Some(request.access_client_id.as_str().into()),
                Some(request.credential_hash.clone()),
                Some(request.peer.to_string()),
            )
            .await
            .map_err(StoreError::backend)?
            .into_iter()
            .map(StoredMachine::decode)
            .collect::<Result<Vec<_>, _>>()?;
            let previous = found
                .iter()
                .find(|machine| machine.origin.installation == request.installation);
            if previous.is_some_and(|machine| machine.peer != request.peer) {
                return Ok((
                    EnrollmentAdmission::Answered(EnrollmentDecision::Refused(
                        EnrollmentRefusal::PeerMismatch,
                    )),
                    Vec::new(),
                ));
            }
            // Check every conflict before any write. An update cannot release an
            // existing unique identity and then fail after partially changing it.
            for machine in &found {
                if machine.origin.installation == request.installation
                    && revocation::in_progress(scope, machine.last_revocation_operation).await?
                {
                    return Ok((
                        EnrollmentAdmission::Answered(EnrollmentDecision::RevocationInProgress),
                        Vec::new(),
                    ));
                }
                if machine.origin.installation != request.installation {
                    let refusal = if machine.peer == request.peer {
                        EnrollmentRefusal::PeerInUse
                    } else if machine.access_client_id == request.access_client_id {
                        EnrollmentRefusal::AccessClientInUse
                    } else if machine.credential_hash == request.credential_hash {
                        EnrollmentRefusal::CredentialHashInUse
                    } else {
                        return Err(corrupt("enrollment lookup"));
                    };
                    return Ok((
                        EnrollmentAdmission::Answered(EnrollmentDecision::Refused(refusal)),
                        Vec::new(),
                    ));
                }
            }
            let peer = request.peer;
            if let Some(machine) = previous {
                if machine.state == EnrollmentState::Enrolled
                    && enrollment_matches(machine, &request)
                {
                    return Ok((
                        EnrollmentAdmission::Answered(EnrollmentDecision::MachineEnrolled {
                            peer,
                            expires_at: machine.expires_at,
                            authoritative_receiver: self.authoritative_receiver,
                            relay_peer: self.relay_peer,
                        }),
                        Vec::new(),
                    ));
                }
                if operation_pending(
                    scope,
                    machine.last_enrollment_operation,
                    readmission::READMISSION_TASK_KIND,
                    "last_enrollment_operation",
                )
                .await?
                {
                    return Ok((
                        if enrollment_matches(machine, &request) {
                            EnrollmentAdmission::Readmitting {
                                operation: machine
                                    .last_enrollment_operation
                                    .ok_or_else(|| corrupt("last_enrollment_operation"))?,
                                peer,
                                expires_at: machine.expires_at,
                                authoritative_receiver: self.authoritative_receiver,
                                relay_peer: self.relay_peer,
                            }
                        } else {
                            EnrollmentAdmission::Answered(EnrollmentDecision::EnrollmentInProgress)
                        },
                        Vec::new(),
                    ));
                }
                if machine.state == EnrollmentState::Revoked {
                    let key = RequestKey::new(format!("central-readmit:{peer}:{}", self.nonce))
                        .map_err(StoreError::backend)?;
                    let admitted = scope
                        .admit::<ReadmissionTask<Store, Driver>>(
                            &key,
                            &readmission::ReadmissionInput { peer },
                        )
                        .await
                        .map_err(StoreError::backend)?;
                    let operation = admitted.id();
                    queries::admit_readmission(
                        scope,
                        request.label.as_str().into(),
                        request.access_client_id.as_str().into(),
                        request.credential_hash,
                        request.expires_at.to_rfc3339(),
                        String::from(operation),
                        self.now.to_rfc3339(),
                        peer.to_string(),
                    )
                    .await
                    .map_err(StoreError::backend)?;
                    scope
                        .ensure_wake(SyncTime::from_unix_millis(self.now.timestamp_millis()))
                        .await?;
                    return Ok((
                        EnrollmentAdmission::Readmitting {
                            operation,
                            peer,
                            expires_at: request.expires_at,
                            authoritative_receiver: self.authoritative_receiver,
                            relay_peer: self.relay_peer,
                        },
                        Vec::new(),
                    ));
                }
            }
            if previous.is_some() {
                queries::update_enrollment(
                    scope,
                    request.label.as_str().into(),
                    request.access_client_id.as_str().into(),
                    request.credential_hash,
                    request.expires_at.to_rfc3339(),
                    self.now.to_rfc3339(),
                    peer.to_string(),
                )
                .await
                .map_err(StoreError::backend)?;
            } else {
                queries::insert_enrollment(
                    scope,
                    peer.to_string(),
                    request.installation.as_str().into(),
                    request.label.as_str().into(),
                    request.access_client_id.as_str().into(),
                    request.credential_hash,
                    request.expires_at.to_rfc3339(),
                    self.now.to_rfc3339(),
                    self.now.to_rfc3339(),
                )
                .await
                .map_err(StoreError::backend)?;
            }
            Ok((
                EnrollmentAdmission::Answered(EnrollmentDecision::MachineEnrolled {
                    peer,
                    expires_at: request.expires_at,
                    authoritative_receiver: self.authoritative_receiver,
                    relay_peer: self.relay_peer,
                }),
                Vec::new(),
            ))
        })
    }
}

struct AuthenticateStep {
    access_client_id: AccessClientId,
    credential_hash: String,
    now: SyncTime,
}
impl<Store, Driver> StepCallback<Store, Driver> for AuthenticateStep
where
    Store: AtomicStore,
    Driver: Database,
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = Result<AdmittedMachine, AuthRefusal>;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let mut found =
                queries::read_enrollments(scope, None, None, Some(self.credential_hash), None)
                    .await
                    .map_err(StoreError::backend)?;
            if found.len() > 1 {
                return Err(corrupt("credential cardinality"));
            }
            let decision = match found.pop().map(StoredMachine::decode).transpose()? {
                None => Err(AuthRefusal::Unknown),
                Some(machine) if machine.state == EnrollmentState::Revoked => {
                    Err(AuthRefusal::Revoked)
                }
                Some(machine)
                    if machine.expires_at.timestamp_millis() <= self.now.unix_millis() =>
                {
                    Err(AuthRefusal::Expired)
                }
                Some(machine) if machine.access_client_id != self.access_client_id => {
                    Err(AuthRefusal::WrongKind)
                }
                Some(machine) => Ok(AdmittedMachine {
                    peer: machine.peer,
                    origin: machine.origin,
                    expires_at: SyncTime::from_unix_millis(machine.expires_at.timestamp_millis()),
                }),
            };
            Ok((decision, Vec::new()))
        })
    }
}

fn enrollment_matches(machine: &StoredMachine, request: &EnrollMachine) -> bool {
    machine.origin.installation == request.installation
        && machine.origin.machine == request.label
        && machine.access_client_id == request.access_client_id
        && machine.credential_hash == request.credential_hash
        && machine.expires_at == request.expires_at
}
async fn operation_pending<Scope: JournalStorage>(
    scope: &mut Scope,
    operation: Option<OperationId>,
    kind: &'static str,
    field: &'static str,
) -> Result<bool, StoreError> {
    let Some(operation) = operation else {
        return Ok(false);
    };
    let record = scope
        .operation(operation)
        .await?
        .ok_or_else(|| corrupt(field))?;
    if record.kind != kind {
        return Err(corrupt(field));
    }
    Ok(!matches!(
        record.execution,
        OperationExecution::Completed { .. }
    ))
}
fn wrong_face() -> StoreError {
    StoreError::Aborted {
        reason: "registry runtime has a different face".into(),
    }
}
