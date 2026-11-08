//! The Fleet object's side of the private sign-in link: provisioning and the
//! renewal digest read, floor delivery, plus the stamp check every other
//! command waits on.
//!
//! These functions decide; [`OrganizationLinkFleet`] is everything they need
//! from the Fleet object that serves them. The object runtime implements it
//! (`fleet_runtime::organization_link_fleet`), so this module never reaches into
//! the runtime, its call envelope or its step driver.
use crate::{
    CentralObservation,
    fleet_organization_authority::{FleetOrganizationAuthority, StampStanding},
    identity_contract::{AuthorizationRevision, ProvisionOrganization},
    organization_authority::{
        ApplyProvisioning, FleetOrganizationStatementRefusal, ProvisioningApplied,
        ProvisioningOutcome, StoredStampRead,
    },
};
use durable_steps::{CallFailure, CallTarget, NotSentReason, Uuid};
use fleet_identity_protocol::OrganizationId;
use runtime_primitives::{Clock, SyncTime};
use serde::{Deserialize, Serialize};
use std::{future::Future, sync::Arc};

#[derive(Serialize, Deserialize)]
pub struct FleetProvisionOrganization;
impl CallTarget for FleetProvisionOrganization {
    const KIND: &'static str = "central.organization-provisioning";
    type Request = ProvisionOrganization;
    type Answer = ProvisioningApplied;
}

#[derive(Serialize, Deserialize)]
pub struct FleetOrganizationStatementDigest;
impl CallTarget for FleetOrganizationStatementDigest {
    const KIND: &'static str = "central.organization-statement-digest";
    type Request = ();
    type Answer = StatementDigestAnswer;
}

#[derive(Serialize, Deserialize)]
pub struct FleetAuthorizationRevision;
impl CallTarget for FleetAuthorizationRevision {
    const KIND: &'static str = "central.organization-authorization-revision";
    type Request = crate::identity_contract::AuthorizationRevision;
    type Answer = FloorDeliveryAnswer;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum FloorDeliveryAnswer {
    Acknowledged {
        revision: crate::identity_contract::FenceRevision,
    },
    Conflict,
    OrganizationFloorMissing,
    OrganizationMismatch,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum StatementDigestAnswer {
    /// Facts from the stored statement as central verified it for this read.
    Current {
        digest: String,
        expires_at: i64,
        signing_key_id: String,
    },
    Unprovisioned,
}

/// What the link's handlers need from the Fleet object that serves them.
pub(crate) trait OrganizationLinkFleet: Sync {
    /// The organization this Fleet object serves.
    fn organization(&self) -> &OrganizationId;
    /// The object's in-memory view of its own stamp.
    fn stamp_authority(&self) -> &FleetOrganizationAuthority;
    fn now(&self) -> SyncTime;
    fn new_id(&self) -> Uuid;
    /// The Fleet's clock, which the applying transaction samples again.
    fn clock(&self) -> Arc<dyn Clock>;
    /// Hand a classification to the host's telemetry, off the request path.
    fn observe(&self, observation: CentralObservation);
    /// Read the stored stamp in its own transaction.
    fn read_stamp(&self) -> impl Future<Output = Result<StoredStampRead, CallFailure>> + Send;
    /// Apply one verified statement in one transaction.
    fn apply_provisioning(
        &self,
        input: ApplyProvisioning,
    ) -> impl Future<Output = Result<ProvisioningOutcome, CallFailure>> + Send;
    /// Apply one pushed fence revision in one transaction.
    fn deliver_floor(
        &self,
        revision: AuthorizationRevision,
        now_ms: i64,
    ) -> impl Future<Output = Result<FloorDeliveryAnswer, CallFailure>> + Send;
    /// Conformance only: a hold point after a call verified and before the
    /// transaction that applies it.
    #[cfg(feature = "conformance")]
    fn before_apply(
        &self,
        kind: &'static str,
    ) -> impl Future<Output = Result<(), CallFailure>> + Send;
    /// Conformance only: whether the next provisioning aborts after its record.
    #[cfg(feature = "conformance")]
    fn take_record_fault(&self) -> bool;
}

/// Check the stored stamp once per process, before any command relies on it.
pub(crate) async fn stamp_standing(
    fleet: &impl OrganizationLinkFleet,
) -> Result<StampStanding, CallFailure> {
    let authority = fleet.stamp_authority();
    if authority.needs_check() {
        let stored = fleet.read_stamp().await?;
        let verified = match &stored {
            StoredStampRead::Unprovisioned => None,
            StoredStampRead::Stamped(stamp) => {
                let check = authority.check().ok_or_else(refused)?;
                Some(check.verify_statement(stamp.statement()).await)
            }
        };
        authority.record(
            &stored,
            verified
                .as_ref()
                .map(|result| result.as_ref().map_err(|refusal| *refusal)),
        );
    }
    Ok(authority.standing(fleet.now()))
}

/// Provision or renew this Fleet's stamp from a statement the link carried.
pub(crate) async fn provision(
    fleet: &impl OrganizationLinkFleet,
    request: ProvisionOrganization,
) -> Result<ProvisioningApplied, CallFailure> {
    let organization = fleet.organization();
    if &request.organization_id != organization {
        return Ok(ProvisioningApplied::OrganizationMismatch);
    }
    let stored_trusted = stamp_standing(fleet).await? != StampStanding::Refused;
    let authority = fleet.stamp_authority();
    let check = authority.check().ok_or_else(refused)?;
    // Signature and profile first, outside the transaction; nothing is
    // written for a statement that does not verify.
    let statement = match check
        .verify_statement(request.organization_statement.as_str())
        .await
    {
        Ok(statement) => statement,
        Err(refusal) => {
            observe_refusal(fleet, refusal);
            return match refusal {
                FleetOrganizationStatementRefusal::VerificationUnavailable(_)
                | FleetOrganizationStatementRefusal::InvalidConfiguration => {
                    Err(CallFailure::NotSent {
                        reason: NotSentReason::Overloaded,
                    })
                }
                FleetOrganizationStatementRefusal::OrganizationMismatch => {
                    Ok(ProvisioningApplied::OrganizationMismatch)
                }
                _ => Ok(ProvisioningApplied::InvalidStatement),
            };
        }
    };
    // Conformance only: hold a verified statement here, before its
    // transaction, so another provisioning can commit in between.
    #[cfg(feature = "conformance")]
    fleet.before_apply(FleetProvisionOrganization::KIND).await?;
    let outcome = fleet
        .apply_provisioning(ApplyProvisioning {
            statement,
            compact: request.organization_statement.as_str().to_owned(),
            record_id: fleet.new_id(),
            clock: fleet.clock(),
            stored_trusted,
            #[cfg(feature = "conformance")]
            fail_after_record: fleet.take_record_fault(),
        })
        .await?;
    if let Some(refusal) = outcome.refusal {
        observe_refusal(fleet, refusal);
    }
    // The committed stamp is the one this process now trusts.
    if outcome.applied == ProvisioningApplied::Provisioned {
        authority.forget();
    }
    Ok(outcome.applied)
}

/// The digest of the statement this Fleet holds, as verified for this read.
pub(crate) async fn statement_digest(
    fleet: &impl OrganizationLinkFleet,
) -> Result<StatementDigestAnswer, CallFailure> {
    match stamp_standing(fleet).await? {
        StampStanding::Unprovisioned => Ok(StatementDigestAnswer::Unprovisioned),
        StampStanding::Refused => Err(refused()),
        StampStanding::Valid => {
            let StoredStampRead::Stamped(stamp) = fleet.read_stamp().await? else {
                return Ok(StatementDigestAnswer::Unprovisioned);
            };
            // Report only what the signature check establishes now; an
            // expired or unverifiable statement can never be described.
            let check = fleet.stamp_authority().check().ok_or_else(refused)?;
            let verified = check
                .verify_statement(stamp.statement())
                .await
                .map_err(|_| refused())?;
            Ok(StatementDigestAnswer::Current {
                digest: verified.digest().as_str().to_owned(),
                expires_at: verified.expires_at().0,
                signing_key_id: verified.key_id().to_owned(),
            })
        }
    }
}

/// Floors only reach a Fleet whose stamp is valid; otherwise the push stays
/// pending at the sign-in service.
pub(crate) async fn authorization_revision(
    fleet: &impl OrganizationLinkFleet,
    revision: AuthorizationRevision,
) -> Result<FloorDeliveryAnswer, CallFailure> {
    if stamp_standing(fleet).await? != StampStanding::Valid {
        return Err(refused());
    }
    if revision.organization_id() != fleet.organization() {
        return Ok(FloorDeliveryAnswer::OrganizationMismatch);
    }
    fleet
        .deliver_floor(revision, fleet.now().unix_millis())
        .await
}

pub(crate) fn refused() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}

/// Hand one statement refusal to the host's telemetry: its closed kind and the
/// organization this Fleet serves, nothing from the statement itself.
fn observe_refusal(fleet: &impl OrganizationLinkFleet, refusal: FleetOrganizationStatementRefusal) {
    fleet.observe(CentralObservation::OrganizationStatementRefused {
        organization: fleet.organization().clone(),
        refusal,
    });
}
