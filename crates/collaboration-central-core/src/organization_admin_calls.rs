//! A person's admin request at the organization's Fleet object: verify the
//! token and proof, then one transaction admits it against the floors, writes
//! the effect through the Fleet's own decision path and records the change.
//!
//! This module decides what happens before that transaction; everything it
//! needs from the Fleet object is [`OrganizationAdminFleet`]. The transaction
//! itself, admission, the decision and the record together, is the runtime's
//! one callback (`fleet_runtime::organization_admin_fleet`), so this module
//! never reaches into the object runtime, its envelope or its step driver.
use crate::{
    fleet_organization_authority::{FleetOrganizationAuthority, StampStanding},
    organization_authority::OrganizationChangeRecord,
    person_request::{
        AdmissionRefusal, PersonRequestInput, PersonRequestRefusal, VerifiedPersonRequest,
    },
};
use durable_steps::{CallFailure, CallTarget, NotSentReason, StoreError};
use fleet_identity_protocol::OrganizationId;
use message_board::{ActorOrigin, Description, ResourceName, WorkspaceId, WorkspaceState};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::future::Future;
#[derive(Serialize, Deserialize)]
pub struct FleetOrganizationAdmin;
impl CallTarget for FleetOrganizationAdmin {
    const KIND: &'static str = "central.organization-admin";
    type Request = PersonRequestInput;
    type Answer = OrganizationAdminAnswer;
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceView {
    pub workspace_id: WorkspaceId,
    pub name: ResourceName,
    pub description: Description,
    pub state: WorkspaceState,
}

/// The Fleet object's answer to the link: a published result, or a
/// verification failure the link turns into a bare 401 that reveals nothing.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OrganizationAdminAnswer {
    Answered(OrganizationAdminResult),
    Unauthenticated,
}

/// The closed body of a 200 on an admin route.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "result",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum OrganizationAdminResult {
    /// `alreadyExisted` when this operation, or another for the same canonical
    /// name, had already created it; the caller sees one workspace either way.
    Applied {
        workspace: WorkspaceView,
        already_existed: bool,
    },
    Found {
        workspace: Option<WorkspaceView>,
    },
    Records {
        records: Vec<OrganizationChangeRecord>,
    },
    Refused {
        reason: AdminRefusal,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum AdminRefusal {
    NotPermitted,
    SecondFactorRequired,
    RefreshRequired,
    AuthorizationSyncRequired,
    Expired,
    RequestTime,
    RequestReused,
    /// The same operation ID arrived with a different request.
    Conflict,
    /// The Fleet's own rules refused the change.
    Rejected,
}

impl From<AdmissionRefusal> for AdminRefusal {
    fn from(refusal: AdmissionRefusal) -> Self {
        match refusal {
            AdmissionRefusal::NotPermitted => Self::NotPermitted,
            AdmissionRefusal::SecondFactorRequired => Self::SecondFactorRequired,
            AdmissionRefusal::RefreshRequired => Self::RefreshRequired,
            AdmissionRefusal::AuthorizationSyncRequired => Self::AuthorizationSyncRequired,
            AdmissionRefusal::Expired => Self::Expired,
            AdmissionRefusal::RequestTime => Self::RequestTime,
            AdmissionRefusal::RequestReused => Self::RequestReused,
        }
    }
}

/// The one place a person's admin record origin is built: the installation
/// that wrote it is the sign-in service (its configured ServiceId, never an
/// organization ID); the record's actor is the person.
pub(crate) fn admin_record_origin(
    sign_in_service: message_board::ServiceId,
) -> Result<ActorOrigin, StoreError> {
    Ok(ActorOrigin {
        installation: sign_in_service,
        machine: "sign-in service"
            .to_owned()
            .try_into()
            .map_err(|error| StoreError::backend(format!("admin origin label: {error}")))?,
    })
}

/// What the admin decision needs from the Fleet object that serves it.
pub(crate) trait OrganizationAdminFleet: Sync {
    /// The organization this Fleet object serves.
    fn organization(&self) -> &OrganizationId;
    /// The object's in-memory view of its own stamp and sign-in trust.
    fn stamp_authority(&self) -> &FleetOrganizationAuthority;
    /// Whether the stored stamp can be relied on right now.
    fn stamp_standing(&self) -> impl Future<Output = Result<StampStanding, CallFailure>> + Send;
    /// The one applying transaction: admission against the floors, then the
    /// effect and its record, on the Fleet clock sampled inside it.
    fn apply(
        &self,
        request: VerifiedPersonRequest,
        sign_in_service: message_board::ServiceId,
    ) -> impl Future<Output = Result<OrganizationAdminResult, CallFailure>> + Send;
    /// Conformance only: a hold point after verification and before the
    /// transaction that admits the request.
    #[cfg(feature = "conformance")]
    fn before_apply(
        &self,
        kind: &'static str,
    ) -> impl Future<Output = Result<(), CallFailure>> + Send;
    /// Conformance only: whether the next admin answer is lost after its commit.
    #[cfg(feature = "conformance")]
    fn take_answer_drop(&self) -> bool;
}

/// Answer one person's admin request.
pub(crate) async fn admin(
    fleet: &impl OrganizationAdminFleet,
    input: PersonRequestInput,
) -> Result<OrganizationAdminAnswer, CallFailure> {
    // An unstamped or untrusted Fleet answers nothing about its subjects.
    if fleet.stamp_standing().await? != StampStanding::Valid {
        return Err(refused());
    }
    let check = fleet.stamp_authority().check().ok_or_else(refused)?;
    // Without the sign-in service's configured identity there is no
    // honest origin to record; refuse rather than invent one.
    let sign_in_service = check.sign_in_service().ok_or(CallFailure::NotSent {
        reason: NotSentReason::Overloaded,
    })?;
    let verified = match check.verify_person_request(&input).await {
        Ok(verified) => verified,
        Err(PersonRequestRefusal::VerificationUnavailable | PersonRequestRefusal::Policy) => {
            return Err(CallFailure::NotSent {
                reason: NotSentReason::Overloaded,
            });
        }
        Err(_) => return Ok(OrganizationAdminAnswer::Unauthenticated),
    };
    // A token for another organization is refused before any read.
    if &verified.organization_id != fleet.organization() {
        return Ok(OrganizationAdminAnswer::Unauthenticated);
    }
    // Conformance only: hold a verified request here, before the
    // transaction that admits it, so the Fleet clock can move in between.
    #[cfg(feature = "conformance")]
    fleet.before_apply(FleetOrganizationAdmin::KIND).await?;
    let answer = fleet.apply(verified, sign_in_service).await?;
    // Conformance only: the change is committed; its answer is lost.
    #[cfg(feature = "conformance")]
    if fleet.take_answer_drop() {
        return Err(CallFailure::AnswerLost);
    }
    Ok(OrganizationAdminAnswer::Answered(answer))
}

pub(crate) fn refused() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}
