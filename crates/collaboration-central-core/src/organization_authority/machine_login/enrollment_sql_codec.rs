//! The staged machine-login tables' rows, checked in Rust on every read and
//! encoded on every write. The tables carry no CHECK constraints (codex-router
//! allows CHECK only for booleans), so every state, value and combination is
//! validated here, and an invalid stored row is refused, never coerced.
use super::enrollment_records::{
    AllowanceRow, EnrollmentGeneration, StoredEnrollment, StoredEnrollmentState,
};
use crate::fleet_row_codec::{check_private_id, corrupt, required};
use crate::generated_b3_registry::{StoredB3Allowance, StoredB3Enrollment, StoredB3Installation};
use crate::identity_contract::{
    AllowanceState, ClientId, DeviceLoginRequestId, EnrollmentRevocationReason, FormerServiceId,
    PersonId,
};
use durable_steps::{StoreError, Uuid};
use fleet_identity_protocol::{EnrollmentId, InstallationId, UnixSeconds};
use message_board::{MachineLabel, WorkspaceId};

type Column = (Option<String>, Option<String>);

fn text((value, class): Column, field: &str) -> Result<String, StoreError> {
    if required(class, field)? != "text" {
        return Err(corrupt(field, "column is not TEXT"));
    }
    required(value, field)
}

fn optional_text((value, class): Column, field: &str) -> Result<Option<String>, StoreError> {
    match required(class, field)?.as_str() {
        "null" => Ok(None),
        "text" => required(value, field).map(Some),
        _ => Err(corrupt(field, "column is neither TEXT nor NULL")),
    }
}

fn integer((value, class): Column, field: &str) -> Result<i64, StoreError> {
    if required(class, field)? != "integer" {
        return Err(corrupt(field, "column is not INTEGER"));
    }
    let value = required(value, field)?;
    let parsed = value
        .parse::<i64>()
        .map_err(|error| corrupt(field, error))?;
    if parsed.to_string() != value {
        return Err(corrupt(field, "integer is not canonical"));
    }
    Ok(parsed)
}

fn optional_integer((value, class): Column, field: &str) -> Result<Option<i64>, StoreError> {
    if required(class.clone(), field)? == "null" {
        return Ok(None);
    }
    integer((value, class), field).map(Some)
}

fn non_negative_millis(value: i64, field: &str) -> Result<i64, StoreError> {
    if value < 0 {
        return Err(corrupt(field, "a time is never negative"));
    }
    Ok(value)
}

/// Seconds the model keeps, stored as whole milliseconds.
fn seconds_from_millis(value: i64, field: &str) -> Result<UnixSeconds, StoreError> {
    if value.rem_euclid(1000) != 0 {
        return Err(corrupt(
            field,
            "a seconds value is stored as whole milliseconds",
        ));
    }
    Ok(UnixSeconds(non_negative_millis(value, field)? / 1000))
}

pub(crate) fn millis_of_seconds(value: UnixSeconds, field: &str) -> Result<i64, StoreError> {
    value
        .0
        .checked_mul(1000)
        .ok_or_else(|| corrupt(field, "seconds outside the millisecond range"))
}

fn generation(value: i64, field: &str) -> Result<EnrollmentGeneration, StoreError> {
    u64::try_from(value)
        .map(EnrollmentGeneration)
        .map_err(|_| corrupt(field, "a generation is never negative"))
}

pub(crate) fn generation_text(
    value: EnrollmentGeneration,
    field: &str,
) -> Result<String, StoreError> {
    i64::try_from(value.0)
        .map(|value| value.to_string())
        .map_err(|_| corrupt(field, "generation outside the stored integer range"))
}

fn uuid_v7_text(column: Column, field: &str) -> Result<String, StoreError> {
    let value = text(column, field)?;
    check_private_id(&value, field)?;
    Ok(value)
}

fn typed<T: TryFrom<String>>(value: String, field: &str) -> Result<T, StoreError> {
    T::try_from(value).map_err(|_| corrupt(field, "value is not valid for its type"))
}

/// One installation: its id, former id, label and generation.
pub(crate) struct DecodedInstallation {
    pub id: InstallationId,
    pub former_service_id: Option<FormerServiceId>,
    pub label: MachineLabel,
    pub generation: EnrollmentGeneration,
}

pub(crate) fn decode_installation(
    row: StoredB3Installation,
) -> Result<DecodedInstallation, StoreError> {
    Ok(DecodedInstallation {
        id: typed(
            uuid_v7_text((row.id, row.id_class), "machine_installations.id")?,
            "machine_installations.id",
        )?,
        former_service_id: optional_text(
            (row.former_service_id, row.former_service_id_class),
            "machine_installations.former_service_id",
        )?
        .map(|value| typed(value, "machine_installations.former_service_id"))
        .transpose()?,
        label: typed(
            text((row.label, row.label_class), "machine_installations.label")?,
            "machine_installations.label",
        )?,
        generation: generation(
            integer(
                (row.enrollment_generation, row.enrollment_generation_class),
                "machine_installations.enrollment_generation",
            )?,
            "machine_installations.enrollment_generation",
        )?,
    })
}

const STATE: &str = "machine_enrollments.state";

/// Every column of an enrollment, checked, then put together into exactly one
/// lifecycle state: any other combination of state, reason, client, last
/// Grant and end is refused.
pub(crate) fn decode_enrollment(row: StoredB3Enrollment) -> Result<StoredEnrollment, StoreError> {
    let id: EnrollmentId = typed(
        uuid_v7_text((row.id, row.id_class), "machine_enrollments.id")?,
        "machine_enrollments.id",
    )?;
    let installation: InstallationId = typed(
        uuid_v7_text(
            (row.installation_id, row.installation_id_class),
            "machine_enrollments.installation_id",
        )?,
        "machine_enrollments.installation_id",
    )?;
    let request_id: DeviceLoginRequestId = typed(
        uuid_v7_text(
            (row.request_id, row.request_id_class),
            "machine_enrollments.request_id",
        )?,
        "machine_enrollments.request_id",
    )?;
    let approved_by: PersonId = typed(
        uuid_v7_text(
            (row.approved_by_person_id, row.approved_by_person_id_class),
            "machine_enrollments.approved_by_person_id",
        )?,
        "machine_enrollments.approved_by_person_id",
    )?;
    if text(
        (row.permission_checked, row.permission_checked_class),
        "machine_enrollments.permission_checked",
    )? != "machine:enroll"
    {
        return Err(corrupt(
            "machine_enrollments.permission_checked",
            "an approval is vouched for machine:enroll only",
        ));
    }
    let base_generation = generation(
        integer(
            (row.base_generation, row.base_generation_class),
            "machine_enrollments.base_generation",
        )?,
        "machine_enrollments.base_generation",
    )?;
    let code_expires_at = seconds_from_millis(
        integer(
            (row.code_expires_at, row.code_expires_at_class),
            "machine_enrollments.code_expires_at",
        )?,
        "machine_enrollments.code_expires_at",
    )?;
    let approved_at_ms = non_negative_millis(
        integer(
            (row.created_at, row.created_at_class),
            "machine_enrollments.created_at",
        )?,
        "machine_enrollments.created_at",
    )?;
    let client = optional_text(
        (row.machine_client_id, row.machine_client_id_class),
        "machine_enrollments.machine_client_id",
    )?
    .map(|value| {
        check_private_id(&value, "machine_enrollments.machine_client_id")?;
        typed::<ClientId>(value, "machine_enrollments.machine_client_id")
    })
    .transpose()?;
    let last_grant = optional_integer(
        (row.last_grant_at, row.last_grant_at_class),
        "machine_enrollments.last_grant_at",
    )?
    .map(|value| seconds_from_millis(value, "machine_enrollments.last_grant_at"))
    .transpose()?;
    let ended_at_ms = optional_integer(
        (row.ended_at, row.ended_at_class),
        "machine_enrollments.ended_at",
    )?
    .map(|value| non_negative_millis(value, "machine_enrollments.ended_at"))
    .transpose()?;
    let end_reason = optional_text(
        (row.end_reason, row.end_reason_class),
        "machine_enrollments.end_reason",
    )?;
    let state = text((row.state, row.state_class), STATE)?;
    let state = match (
        state.as_str(),
        end_reason.as_deref(),
        client,
        last_grant,
        ended_at_ms,
    ) {
        ("approved", None, None, None, None) => StoredEnrollmentState::Approved,
        ("lapsed", None, None, None, Some(_)) => StoredEnrollmentState::Lapsed,
        ("enrolled", None, Some(machine_client_id), Some(last_grant_at), None) => {
            StoredEnrollmentState::Enrolled {
                machine_client_id,
                last_grant_at,
            }
        }
        ("revoked", Some(reason), Some(machine_client_id), _, Some(_)) => {
            StoredEnrollmentState::Revoked {
                machine_client_id,
                reason: match reason {
                    "owner" => EnrollmentRevocationReason::Owner {},
                    "superseded" => EnrollmentRevocationReason::Superseded {},
                    _ => return Err(corrupt("machine_enrollments.end_reason", "unknown reason")),
                },
            }
        }
        ("expired", None, Some(machine_client_id), _, Some(_)) => {
            StoredEnrollmentState::Expired { machine_client_id }
        }
        _ => {
            return Err(corrupt(
                STATE,
                "state, reason, client, last Grant and end don't form one lifecycle state",
            ));
        }
    };
    Ok(StoredEnrollment {
        id,
        installation,
        request_id,
        approved_by,
        base_generation,
        approved_at_ms,
        code_expires_at,
        state,
        ended_at_ms,
    })
}

/// The mutable columns of an enrollment, as written.
pub(crate) struct EncodedEnrollmentState {
    pub state: &'static str,
    pub end_reason: Option<String>,
    pub machine_client_id: Option<String>,
    pub last_grant_at: Option<String>,
}

pub(crate) fn encode_state(
    state: &StoredEnrollmentState,
) -> Result<EncodedEnrollmentState, StoreError> {
    let client = |id: &ClientId| Some(id.as_str().to_owned());
    Ok(match state {
        StoredEnrollmentState::Approved => EncodedEnrollmentState {
            state: "approved",
            end_reason: None,
            machine_client_id: None,
            last_grant_at: None,
        },
        StoredEnrollmentState::Lapsed => EncodedEnrollmentState {
            state: "lapsed",
            end_reason: None,
            machine_client_id: None,
            last_grant_at: None,
        },
        StoredEnrollmentState::Enrolled {
            machine_client_id,
            last_grant_at,
        } => EncodedEnrollmentState {
            state: "enrolled",
            end_reason: None,
            machine_client_id: client(machine_client_id),
            last_grant_at: Some(
                millis_of_seconds(*last_grant_at, "machine_enrollments.last_grant_at")?.to_string(),
            ),
        },
        StoredEnrollmentState::Revoked {
            machine_client_id,
            reason,
        } => EncodedEnrollmentState {
            state: "revoked",
            end_reason: Some(
                match reason {
                    EnrollmentRevocationReason::Owner {} => "owner",
                    EnrollmentRevocationReason::Superseded {} => "superseded",
                }
                .to_owned(),
            ),
            machine_client_id: client(machine_client_id),
            last_grant_at: None,
        },
        StoredEnrollmentState::Expired { machine_client_id } => EncodedEnrollmentState {
            state: "expired",
            end_reason: None,
            machine_client_id: client(machine_client_id),
            last_grant_at: None,
        },
    })
}

pub(crate) fn decode_allowance(row: StoredB3Allowance) -> Result<AllowanceRow, StoreError> {
    let id = uuid_v7_text((row.id, row.id_class), "machine_allowances.id")?;
    Ok(AllowanceRow {
        id: Uuid::parse_str(&id).map_err(|error| corrupt("machine_allowances.id", error))?,
        installation: typed(
            uuid_v7_text(
                (row.installation_id, row.installation_id_class),
                "machine_allowances.installation_id",
            )?,
            "machine_allowances.installation_id",
        )?,
        workspace: typed::<WorkspaceId>(
            uuid_v7_text(
                (row.workspace_id, row.workspace_id_class),
                "machine_allowances.workspace_id",
            )?,
            "machine_allowances.workspace_id",
        )?,
        state: match text((row.state, row.state_class), "machine_allowances.state")?.as_str() {
            "active" => AllowanceState::Active,
            "withdrawn" => AllowanceState::Withdrawn,
            _ => {
                return Err(corrupt(
                    "machine_allowances.state",
                    "unknown allowance state",
                ));
            }
        },
    })
}

pub(crate) const fn allowance_state_text(state: &AllowanceState) -> &'static str {
    match state {
        AllowanceState::Active => "active",
        AllowanceState::Withdrawn => "withdrawn",
    }
}
