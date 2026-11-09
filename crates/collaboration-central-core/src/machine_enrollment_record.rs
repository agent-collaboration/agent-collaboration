//! Decode enrollment storage into validated identities without depending on workflows.
use crate::{AccessClientId, AdmissionOrigin, generated_enrollment as queries};
use chrono::{DateTime, Utc};
use durable_steps::{OperationId, StoreError};
use fleet_sync_protocol::PeerId;
use message_board::{MachineLabel, ServiceId};
use runtime_primitives::SyncTime;
#[cfg(feature = "conformance")]
use serde::Serialize;

#[derive(Eq, PartialEq)]
#[cfg_attr(feature = "conformance", derive(Serialize))]
pub(crate) enum EnrollmentState {
    Enrolled,
    Revoked,
}
#[cfg_attr(feature = "conformance", derive(Serialize))]
pub(crate) struct StoredMachine {
    pub(crate) peer: PeerId,
    pub(crate) origin: AdmissionOrigin,
    pub(crate) access_client_id: AccessClientId,
    pub(crate) credential_hash: String,
    pub(crate) state: EnrollmentState,
    pub(crate) expires_at: DateTime<Utc>,
    pub(crate) last_seen: Option<SyncTime>,
    pub(crate) last_revocation_operation: Option<durable_steps::OperationId>,
    pub(crate) last_enrollment_operation: Option<OperationId>,
}
impl StoredMachine {
    pub(crate) fn decode(row: queries::StoredEnrollment) -> Result<Self, StoreError> {
        let _: DateTime<Utc> = required(row.created_at, "created_at")?
            .parse::<DateTime<Utc>>()
            .map_err(|_| corrupt("created_at"))?;
        let _: DateTime<Utc> = required(row.updated_at, "updated_at")?
            .parse::<DateTime<Utc>>()
            .map_err(|_| corrupt("updated_at"))?;
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

pub(crate) fn valid_hash(value: &str) -> bool {
    !value.is_empty() && value.trim() == value && !value.chars().any(char::is_control)
}
fn required(value: Option<String>, field: &str) -> Result<String, StoreError> {
    value.ok_or_else(|| corrupt(field))
}
pub(crate) fn corrupt(field: &str) -> StoreError {
    StoreError::Corrupt {
        field: format!("machine_enrollments.{field}"),
        message: "invalid stored enrollment".into(),
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

/// The journal kinds of today's revocation and readmission operations. B3's
/// switch refuses while any is unfinished, so they are named once, here.
pub(crate) const REVOKE_TASK_KIND: &str = "central.revoke-machine";
pub(crate) const READMISSION_TASK_KIND: &str = "central.readmit-machine";
