//! The registry owns the stable mapping from a Router identity to a library peer.

use crate::{
    AccessClientId, AdmissionOrigin, ObjectHost, ObjectRuntime, OwnerIdentity, PlatformIo,
    RegistryKind, generated_enrollment as queries,
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
    /// Reserved identities become active after the mailbox has opened.
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
    pub async fn enroll_machine(
        &self,
        _owner: &OwnerIdentity,
        request: EnrollMachine,
    ) -> Result<EnrollmentDecision, StoreError> {
        let ObjectWork::Registry {
            host,
            shared_host,
            engine,
            delivery,
            ..
        } = &self.work
        else {
            return Err(wrong_face());
        };
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
        let ObjectWork::Registry {
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
                if machine.state == EnrollmentState::Enrolled && machine.matches(&request) {
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
                        if machine.matches(&request) {
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

#[derive(Eq, PartialEq)]
#[cfg_attr(feature = "conformance", derive(Serialize))]
enum EnrollmentState {
    Enrolled,
    Revoked,
}
#[cfg_attr(feature = "conformance", derive(Serialize))]
struct StoredMachine {
    peer: PeerId,
    origin: AdmissionOrigin,
    access_client_id: AccessClientId,
    credential_hash: String,
    state: EnrollmentState,
    expires_at: DateTime<Utc>,
    last_seen: Option<SyncTime>,
    last_revocation_operation: Option<durable_steps::OperationId>,
    last_enrollment_operation: Option<OperationId>,
}
impl StoredMachine {
    fn matches(&self, request: &EnrollMachine) -> bool {
        self.origin.installation == request.installation
            && self.origin.machine == request.label
            && self.access_client_id == request.access_client_id
            && self.credential_hash == request.credential_hash
            && self.expires_at == request.expires_at
    }
    fn decode(row: queries::StoredEnrollment) -> Result<Self, StoreError> {
        let peer =
            PeerId::try_from(required(row.peer_id, "peer_id")?).map_err(|_| corrupt("peer_id"))?;
        let installation = ServiceId::try_from(required(row.service_id, "service_id")?)
            .map_err(|_| corrupt("service_id"))?;
        let label = required(row.machine_label, "machine_label")?;
        let machine =
            MachineLabel::try_from(label.clone()).map_err(|_| corrupt("machine_label"))?;
        if machine.as_str() != label {
            return Err(corrupt("machine_label"));
        }
        let access_client_id =
            AccessClientId::new(required(row.access_client_id, "access_client_id")?)
                .map_err(|_| corrupt("access_client_id"))?;
        let credential_hash = required(row.credential_hash, "credential_hash")?;
        if !valid_hash(&credential_hash) {
            return Err(corrupt("credential_hash"));
        }
        let state = match required(row.state, "state")?.as_str() {
            "enrolled" => EnrollmentState::Enrolled,
            "revoked" => EnrollmentState::Revoked,
            _ => return Err(corrupt("state")),
        };
        let expires_at = required(row.expires_at, "expires_at")?
            .parse()
            .map_err(|_| corrupt("expires_at"))?;
        let last_seen = stored_presence(row.last_seen)?;
        let last_revocation_operation = row
            .last_revocation_operation
            .map(durable_steps::OperationId::try_from)
            .transpose()
            .map_err(|_| corrupt("last_revocation_operation"))?;
        let last_enrollment_operation = row
            .last_enrollment_operation
            .map(OperationId::try_from)
            .transpose()
            .map_err(|_| corrupt("last_enrollment_operation"))?;
        Ok(Self {
            peer,
            origin: AdmissionOrigin {
                installation,
                machine,
            },
            access_client_id,
            credential_hash,
            state,
            expires_at,
            last_seen,
            last_revocation_operation,
            last_enrollment_operation,
        })
    }
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
fn valid_hash(value: &str) -> bool {
    !value.is_empty() && value.trim() == value && !value.chars().any(char::is_control)
}
fn required(value: Option<String>, field: &str) -> Result<String, StoreError> {
    value.ok_or_else(|| corrupt(field))
}
fn corrupt(field: &str) -> StoreError {
    StoreError::Corrupt {
        field: format!("machine_enrollments.{field}"),
        message: "invalid stored enrollment".into(),
    }
}
fn wrong_face() -> StoreError {
    StoreError::Aborted {
        reason: "registry runtime has a different face".into(),
    }
}

fn stored_presence(value: Option<String>) -> Result<Option<SyncTime>, StoreError> {
    value
        .map(|value| {
            let parsed: DateTime<Utc> = value.parse().map_err(|_| corrupt("last_seen"))?;
            let millis = parsed.timestamp_millis();
            let exact =
                DateTime::from_timestamp_millis(millis).ok_or_else(|| corrupt("last_seen"))?;
            if exact != parsed {
                return Err(corrupt("last_seen"));
            }
            Ok(SyncTime::from_unix_millis(millis))
        })
        .transpose()
}

pub(crate) fn service_for_stored_peer(
    row: queries::StoredEnrollment,
    expected: PeerId,
) -> Result<ServiceId, StoreError> {
    let machine = StoredMachine::decode(row)?;
    if machine.peer != expected {
        return Err(corrupt("peer_id"));
    }
    Ok(machine.origin.installation)
}

/// Identity and Registry presence stay readable after revocation; authentication
/// and submit-time revocation decisions remain with their existing owners.
pub(crate) struct EnrollmentIdentity {
    pub peer: PeerId,
    pub origin: AdmissionOrigin,
    pub last_seen: Option<SyncTime>,
}

pub(crate) fn identity_from_stored_enrollment(
    row: queries::StoredEnrollment,
) -> Result<EnrollmentIdentity, StoreError> {
    let machine = StoredMachine::decode(row)?;
    Ok(EnrollmentIdentity {
        peer: machine.peer,
        origin: machine.origin,
        last_seen: machine.last_seen,
    })
}
