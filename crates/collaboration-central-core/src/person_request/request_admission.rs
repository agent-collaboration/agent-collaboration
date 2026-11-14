//! Admission policy evaluated against one organization-turn clock sample.
use super::{
    compact_jwt::MAX_SAFE_INTEGER, request_verifier::VerifiedPersonRequest,
    request_verifier::in_request_window,
};
use central_storage::StoreFailure;
use serde::{Deserialize, Serialize};

/// Changes need a second factor passed within this many seconds (F2a's window).
pub const SECOND_FACTOR_WINDOW_SECONDS: i64 = 900;

/// A fence revision: a positive JavaScript-safe integer.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct FloorRevision(i64);
impl FloorRevision {
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}
impl TryFrom<i64> for FloorRevision {
    type Error = StoreFailure;
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
    pub(crate) fn parse(value: &str) -> Result<Self, StoreFailure> {
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
    pub(crate) fn parse(value: &str) -> Result<Self, StoreFailure> {
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

/// The admission sample; the reservation and effect must share one batch.
#[derive(Debug)]
pub struct AdmittedRequest {
    pub now_ms: i64,
}

/// Decide before constructing the atomic fresh-proof reservation.
pub(crate) fn decide_admission(
    request: &VerifiedPersonRequest,
    now_ms: i64,
    organization_floor: Option<&OrganizationFloor>,
    person_floor: Option<&PersonFloor>,
    request_used: bool,
) -> Result<Result<AdmittedRequest, AdmissionRefusal>, StoreFailure> {
    if let Err(refusal) = admission_before_person(request, now_ms, organization_floor)? {
        return Ok(Err(refusal));
    }
    let Some(person_floor) = person_floor else {
        return Ok(Err(AdmissionRefusal::AuthorizationSyncRequired));
    };
    if person_floor.subject_id != request.authorization_subject_id {
        return Ok(Err(AdmissionRefusal::NotPermitted));
    }
    let token_person_revision = FloorRevision::try_from(request.person_revision)
        .map_err(|_| corrupt("minimum_revision", "verified token revision out of range"))?;
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
    if request_used {
        return Ok(Err(AdmissionRefusal::RequestReused));
    }
    Ok(Ok(AdmittedRequest { now_ms }))
}

pub(crate) fn request_time_refusal(
    request: &VerifiedPersonRequest,
    now_ms: i64,
) -> Option<AdmissionRefusal> {
    if now_ms >= request.effective_expiry_ms {
        return Some(AdmissionRefusal::Expired);
    }
    if !in_request_window(request.request_time_ms, now_ms)
        || !in_request_window(request.proof_time_ms, now_ms)
    {
        return Some(AdmissionRefusal::RequestTime);
    }
    None
}

pub(crate) fn admission_before_person(
    request: &VerifiedPersonRequest,
    now_ms: i64,
    organization_floor: Option<&OrganizationFloor>,
) -> Result<Result<(), AdmissionRefusal>, StoreFailure> {
    if let Some(refusal) = request_time_refusal(request, now_ms) {
        return Ok(Err(refusal));
    }
    // A missing or corrupt floor never admits; a token never creates one.
    let Some(organization_floor) = organization_floor else {
        return Ok(Err(AdmissionRefusal::AuthorizationSyncRequired));
    };
    if request.request_time_ms <= organization_floor.reject_through_ms {
        return Ok(Err(AdmissionRefusal::RequestTime));
    }
    let token_organization_revision = FloorRevision::try_from(request.organization_revision)
        .map_err(|_| corrupt("minimum_revision", "verified token revision out of range"))?;
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
    Ok(Ok(()))
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

fn corrupt(field: &str, _message: impl std::fmt::Display) -> StoreFailure {
    crate::workspace_decode_error::corrupt(&format!("authorization_floors.{field}"))
}

#[cfg(test)]
#[path = "request_admission_test.rs"]
mod tests;
