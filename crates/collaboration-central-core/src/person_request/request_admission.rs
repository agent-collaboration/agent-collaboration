//! The applying transaction's admission of a verified person request: the
//! pushed floors, the permission, every time bound on the transaction's own
//! clock sample, and the fresh request marker, all before any effect.
use super::{
    compact_jwt::MAX_SAFE_INTEGER, request_verifier::VerifiedPersonRequest,
    request_verifier::in_request_window,
};
use crate::generated_organization as queries;
use durable_steps::{StoreError, storage::TransactionScope};
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

/// Changes need a second factor passed within this many seconds (F2a's window).
pub const SECOND_FACTOR_WINDOW_SECONDS: i64 = 900;

/// A fence revision: a positive JavaScript-safe integer.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct FloorRevision(i64);
impl TryFrom<i64> for FloorRevision {
    type Error = StoreError;
    fn try_from(value: i64) -> Result<Self, Self::Error> {
        if !(1..=MAX_SAFE_INTEGER).contains(&value) {
            return Err(corrupt("minimum_revision", "revision outside 1..=2^53-1"));
        }
        Ok(Self(value))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OrganizationFloorState {
    Active,
    Stopped,
    Deleted,
}
impl OrganizationFloorState {
    pub(crate) fn parse(value: &str) -> Result<Self, StoreError> {
        match value {
            "active" => Ok(Self::Active),
            "stopped" => Ok(Self::Stopped),
            "deleted" => Ok(Self::Deleted),
            _ => Err(corrupt("state", "unknown organization floor state")),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PersonFloorState {
    Active,
    Inactive,
}
impl PersonFloorState {
    pub(crate) fn parse(value: &str) -> Result<Self, StoreError> {
        match value {
            "active" => Ok(Self::Active),
            "inactive" => Ok(Self::Inactive),
            _ => Err(corrupt("state", "unknown person floor state")),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrganizationFloor {
    pub revision: FloorRevision,
    pub state: OrganizationFloorState,
    pub policy_digest: String,
    pub reject_through_ms: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersonFloor {
    pub subject_id: String,
    pub revision: FloorRevision,
    pub state: PersonFloorState,
}

/// Why admission refused; each happens before any marker, ledger or domain write.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "camelCase")]
pub enum AdmissionRefusal {
    #[error("the person's organization role does not grant this operation")]
    NotPermitted,
    #[error("this change needs a fresh second factor")]
    SecondFactorRequired,
    #[error("the token predates the floor central holds; refresh it")]
    RefreshRequired,
    #[error("central has not yet received the authorization the token names")]
    AuthorizationSyncRequired,
    #[error("the token, session or snapshot deadline has passed")]
    Expired,
    #[error("the request's time is outside its window or at or below the watermark")]
    RequestTime,
    #[error("the request ID was already used")]
    RequestReused,
}

/// The admitted request; the caller's effect runs in the same transaction.
#[derive(Debug)]
pub struct AdmittedRequest {
    pub now_ms: i64,
}

/// Run inside the applying transaction, with `now_ms` sampled inside it.
pub(crate) async fn admit<S: TransactionScope>(
    scope: &mut S,
    request: &VerifiedPersonRequest,
    now_ms: i64,
) -> Result<Result<AdmittedRequest, AdmissionRefusal>, StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> String: Type<S::Database> + Encode<'v, S::Database>,
    for<'v> Option<String>: Type<S::Database> + Decode<'v, S::Database> + Encode<'v, S::Database>,
    for<'r> &'r str: ColumnIndex<<S::Database as Database>::Row>,
{
    // Time first: an expired or stale request reserves nothing.
    if now_ms >= request.effective_expiry_ms {
        return Ok(Err(AdmissionRefusal::Expired));
    }
    if !in_request_window(request.request_time_ms, now_ms)
        || !in_request_window(request.proof_time_ms, now_ms)
    {
        return Ok(Err(AdmissionRefusal::RequestTime));
    }
    let organization = request.organization_id.as_str();
    // A missing or corrupt floor never admits; a token never creates one.
    let Some(organization_floor) = read_organization_floor(scope, organization).await? else {
        return Ok(Err(AdmissionRefusal::AuthorizationSyncRequired));
    };
    if request.request_time_ms <= organization_floor.reject_through_ms {
        return Ok(Err(AdmissionRefusal::RequestTime));
    }
    let token_organization_revision = FloorRevision::try_from(request.organization_revision)
        .map_err(|_| StoreError::backend("verified token revision out of range"))?;
    match organization_floor.state {
        OrganizationFloorState::Deleted => return Ok(Err(AdmissionRefusal::NotPermitted)),
        _ if token_organization_revision > organization_floor.revision => {
            return Ok(Err(AdmissionRefusal::AuthorizationSyncRequired));
        }
        OrganizationFloorState::Stopped => return Ok(Err(AdmissionRefusal::NotPermitted)),
        OrganizationFloorState::Active
            if token_organization_revision < organization_floor.revision =>
        {
            return Ok(Err(AdmissionRefusal::RefreshRequired));
        }
        OrganizationFloorState::Active => {}
    }
    if request.permission_policy_digest != organization_floor.policy_digest {
        return Ok(Err(AdmissionRefusal::RefreshRequired));
    }
    let Some(person_floor) = read_person_floor(scope, organization, &request.person_id).await?
    else {
        return Ok(Err(AdmissionRefusal::AuthorizationSyncRequired));
    };
    if person_floor.subject_id != request.authorization_subject_id {
        return Ok(Err(AdmissionRefusal::NotPermitted));
    }
    let token_person_revision = FloorRevision::try_from(request.person_revision)
        .map_err(|_| StoreError::backend("verified token revision out of range"))?;
    if token_person_revision > person_floor.revision {
        return Ok(Err(AdmissionRefusal::AuthorizationSyncRequired));
    }
    match person_floor.state {
        PersonFloorState::Inactive => return Ok(Err(AdmissionRefusal::NotPermitted)),
        PersonFloorState::Active if token_person_revision < person_floor.revision => {
            return Ok(Err(AdmissionRefusal::RefreshRequired));
        }
        PersonFloorState::Active => {}
    }
    let route = request.operation.route();
    if !request
        .permissions
        .iter()
        .any(|granted| granted == route.permission())
    {
        return Ok(Err(AdmissionRefusal::NotPermitted));
    }
    if route.changes_state() && !factor_fresh(request.second_factor_at, now_ms) {
        return Ok(Err(AdmissionRefusal::SecondFactorRequired));
    }
    // The fresh request ID is reserved once, in this transaction.
    let request_id = request.request_id.to_string();
    if queries::used_request_exists(scope, request_id.clone())
        .await
        .map_err(StoreError::backend)?
    {
        return Ok(Err(AdmissionRefusal::RequestReused));
    }
    queries::insert_used_request(
        scope,
        request_id,
        organization.into(),
        request.request_time_ms.to_string(),
        request
            .operation
            .operation_id()
            .map(|id| id.as_str().to_owned()),
        request.person_id.clone(),
        request.session_id.clone(),
        request.intent_digest.clone(),
        request.proof_key_thumbprint.clone(),
        now_ms.to_string(),
    )
    .await
    .map_err(StoreError::backend)?;
    Ok(Ok(AdmittedRequest { now_ms }))
}

fn factor_fresh(second_factor_at: Option<i64>, now_ms: i64) -> bool {
    let Some(age_ms) = second_factor_at
        .and_then(|at| at.checked_mul(1000))
        .and_then(|at| now_ms.checked_sub(at))
    else {
        return false;
    };
    (0..=SECOND_FACTOR_WINDOW_SECONDS * 1000).contains(&age_ms)
}

pub(crate) async fn read_organization_floor<S: TransactionScope>(
    scope: &mut S,
    organization: &str,
) -> Result<Option<OrganizationFloor>, StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> String: Type<S::Database> + Encode<'v, S::Database>,
    for<'v> Option<String>: Type<S::Database> + Decode<'v, S::Database>,
    for<'r> &'r str: ColumnIndex<<S::Database as Database>::Row>,
{
    let Some(row) = queries::read_organization_authorization_floor(scope, organization.into())
        .await
        .map_err(StoreError::backend)?
    else {
        return Ok(None);
    };
    if text(row.id, row.id_class, "id")? != organization {
        return Err(corrupt("id", "floor does not belong to its organization"));
    }
    Ok(Some(OrganizationFloor {
        revision: FloorRevision::try_from(integer(
            row.minimum_revision,
            row.minimum_revision_class,
            "minimum_revision",
        )?)?,
        state: OrganizationFloorState::parse(&text(row.state, row.state_class, "state")?)?,
        policy_digest: non_empty(text(
            row.permission_policy_digest,
            row.permission_policy_digest_class,
            "permission_policy_digest",
        )?)?,
        reject_through_ms: {
            let value = integer(
                row.request_reject_through_ms,
                row.request_reject_through_ms_class,
                "request_reject_through_ms",
            )?;
            if !(0..=MAX_SAFE_INTEGER).contains(&value) {
                return Err(corrupt(
                    "request_reject_through_ms",
                    "watermark out of range",
                ));
            }
            value
        },
    }))
}

pub(crate) async fn read_person_floor<S: TransactionScope>(
    scope: &mut S,
    organization: &str,
    person: &str,
) -> Result<Option<PersonFloor>, StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> String: Type<S::Database> + Encode<'v, S::Database>,
    for<'v> Option<String>: Type<S::Database> + Decode<'v, S::Database>,
    for<'r> &'r str: ColumnIndex<<S::Database as Database>::Row>,
{
    let Some(row) =
        queries::read_person_authorization_floor(scope, organization.into(), person.into())
            .await
            .map_err(StoreError::backend)?
    else {
        return Ok(None);
    };
    let subject_id = text(row.id, row.id_class, "id")?;
    crate::fleet_row_codec::check_private_id(
        &subject_id,
        "person_organization_authorization_floors.id",
    )?;
    if text(
        row.organization_id,
        row.organization_id_class,
        "organization_id",
    )? != organization
        || text(row.person_id, row.person_id_class, "person_id")? != person
    {
        return Err(corrupt("person_id", "floor does not belong to its pair"));
    }
    Ok(Some(PersonFloor {
        subject_id,
        revision: FloorRevision::try_from(integer(
            row.minimum_revision,
            row.minimum_revision_class,
            "minimum_revision",
        )?)?,
        state: PersonFloorState::parse(&text(row.state, row.state_class, "state")?)?,
    }))
}

fn text(value: Option<String>, class: Option<String>, field: &str) -> Result<String, StoreError> {
    if crate::fleet_row_codec::required(class, field)? != "text" {
        return Err(corrupt(field, "column is not TEXT"));
    }
    crate::fleet_row_codec::required(value, field)
}

fn integer(value: Option<String>, class: Option<String>, field: &str) -> Result<i64, StoreError> {
    if crate::fleet_row_codec::required(class, field)? != "integer" {
        return Err(corrupt(field, "column is not INTEGER"));
    }
    let value = crate::fleet_row_codec::required(value, field)?;
    let parsed = value
        .parse::<i64>()
        .map_err(|error| corrupt(field, error))?;
    if parsed.to_string() != value {
        return Err(corrupt(field, "integer is not canonical"));
    }
    Ok(parsed)
}

fn non_empty(value: String) -> Result<String, StoreError> {
    if value.is_empty() {
        return Err(corrupt(
            "permission_policy_digest",
            "policy digest is empty",
        ));
    }
    Ok(value)
}

fn corrupt(field: &str, message: impl std::fmt::Display) -> StoreError {
    StoreError::Corrupt {
        field: format!("authorization_floors.{field}"),
        message: message.to_string(),
    }
}

#[cfg(test)]
#[path = "request_admission_test.rs"]
mod tests;
