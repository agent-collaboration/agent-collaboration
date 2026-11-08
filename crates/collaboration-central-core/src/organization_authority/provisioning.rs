//! Provisioning stamps a Fleet store once, and swaps its statement only by CAS.
use super::fleet_statement_wire::ParsedFleetStatement;
use super::{
    CurrentStamp, OrganizationChangeKind, OrganizationChangeOrigin, OrganizationChangeRecord,
    OrganizationStatementReplacement, OrganizationStatementReplacementRefusal, StoredStampRead,
    VerifiedFleetOrganizationStatement, change_records, decide_organization_statement_replacement,
    organization_stamp_store,
};
use crate::identity_contract::{
    OrganizationStatementDigestReply, ProvisionOrganization, ProvisionOrganizationResult,
    ReadOrganizationStatementDigest,
};
use durable_steps::{StoreError, Uuid, storage::TransactionScope};
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::future::Future;

/// The private link asks the organization's Fleet object to apply provisioning.
pub trait OrganizationProvisioningPort: Send + Sync + 'static {
    /// Return Provisioned only after the stamp and its record have committed together.
    fn provision_organization(
        &self,
        request: &ProvisionOrganization,
    ) -> impl Future<Output = Result<ProvisionOrganizationResult, OrganizationProvisioningFailure>> + Send;
    /// Read only the digest of the statement this organization's Fleet holds.
    fn read_statement_digest(
        &self,
        request: &ReadOrganizationStatementDigest,
    ) -> impl Future<
        Output = Result<OrganizationStatementDigestReply, OrganizationProvisioningFailure>,
    > + Send;
    /// Apply one pushed fence revision; the reply is the retained floor.
    fn deliver_authorization_revision(
        &self,
        revision: &crate::identity_contract::AuthorizationRevision,
    ) -> impl Future<
        Output = Result<
            crate::identity_contract::AuthorizationRevisionAcknowledgement,
            OrganizationProvisioningFailure,
        >,
    > + Send;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum OrganizationProvisioningFailure {
    #[error("the request does not belong to this Fleet object")]
    OrganizationMismatch,
    #[error("the organization statement failed verification")]
    InvalidStatement,
    #[error("the statement names a predecessor the Fleet no longer holds")]
    StaleStatement,
    #[error("the Fleet object has not been provisioned")]
    Unprovisioned,
    #[error("a person floor arrived before its organization floor")]
    OrganizationFloorMissing,
    #[error("organization provisioning is unavailable")]
    Unavailable,
    #[error("organization provisioning could not confirm its commit")]
    OutcomeUnknown,
}

/// What the Fleet object answers over its call boundary; refusals are closed.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ProvisioningApplied {
    Provisioned,
    OrganizationMismatch,
    InvalidStatement,
    StaleStatement,
}

/// One transaction: read the stamp, decide, then write the stamp and its record.
pub(crate) struct ApplyProvisioning {
    pub statement: VerifiedFleetOrganizationStatement,
    pub compact: String,
    pub record_id: Uuid,
    /// Sampled inside the applying transaction: the statement must still be
    /// valid when it is applied, not only when it was verified.
    pub clock: std::sync::Arc<dyn runtime_primitives::Clock>,
    /// False when the stored stamp failed its check since this process started:
    /// then only a replacement naming it is accepted, and a setup retry is
    /// refused instead of answering Provisioned over a stamp nothing trusts.
    pub stored_trusted: bool,
    /// Conformance only: abort after the record write to prove one atomic unit.
    #[cfg(feature = "conformance")]
    pub fail_after_record: bool,
}

#[derive(Debug)]
pub(crate) struct ProvisioningOutcome {
    pub applied: ProvisioningApplied,
    /// The statement's own refusal when the applying transaction refused it.
    pub refusal: Option<super::FleetOrganizationStatementRefusal>,
}

pub(crate) async fn apply<S: TransactionScope>(
    scope: &mut S,
    input: ApplyProvisioning,
) -> Result<ProvisioningOutcome, StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> String: Type<S::Database> + Encode<'v, S::Database>,
    for<'v> Option<String>: Type<S::Database> + Decode<'v, S::Database> + Encode<'v, S::Database>,
    for<'r> &'r str: ColumnIndex<<S::Database as Database>::Row>,
{
    // A statement verified before a wait (another provisioning, a slow queue)
    // may have expired since; decide on the time this transaction runs at.
    let now = input.clock.now();
    if let Err(refusal) = input.statement.require_valid_at(now) {
        return Ok(ProvisioningOutcome {
            applied: ProvisioningApplied::InvalidStatement,
            refusal: Some(refusal),
        });
    }
    let now_millis = now.unix_millis();
    let stored = organization_stamp_store::read(scope).await?;
    let current = match &stored {
        StoredStampRead::Unprovisioned => None,
        StoredStampRead::Stamped(stamp) => Some(CurrentStamp::of_stored(stamp)),
    };
    let organization = input.statement.organization_id().clone();
    let decision = decide_organization_statement_replacement(current.as_ref(), &input.statement);
    let (kind, request_id) = match decision {
        Err(OrganizationStatementReplacementRefusal::OrganizationMismatch) => {
            return Ok(unchanged(ProvisioningApplied::OrganizationMismatch, stored));
        }
        Err(OrganizationStatementReplacementRefusal::StaleOrganizationStatement) => {
            return Ok(unchanged(ProvisioningApplied::StaleStatement, stored));
        }
        // Provisioned means a usable stored stamp at apply time: key trust was
        // sampled outside (`stored_trusted`), expiry is decided here, at `now`.
        Ok(OrganizationStatementReplacement::AlreadyProvisioned)
            if !input.stored_trusted || !stored_unexpired_at(&stored, now) =>
        {
            return Ok(unchanged(ProvisioningApplied::StaleStatement, stored));
        }
        Ok(OrganizationStatementReplacement::AlreadyProvisioned) => {
            return Ok(unchanged(ProvisioningApplied::Provisioned, stored));
        }
        Ok(OrganizationStatementReplacement::Initialize) => {
            organization_stamp_store::insert(scope, &organization, &input.compact, now_millis)
                .await?;
            // Setup carries only the organization ID; a retry finds this record.
            (
                OrganizationChangeKind::OrganizationProvisioned,
                organization.as_str().to_owned(),
            )
        }
        Ok(OrganizationStatementReplacement::Replace) => {
            let StoredStampRead::Stamped(previous) = &stored else {
                return Err(StoreError::Aborted {
                    reason: "a replacement decision requires a stored stamp".into(),
                });
            };
            organization_stamp_store::replace(scope, previous, &input.compact, now_millis).await?;
            // A renewal is identified by the statement it installs.
            (
                OrganizationChangeKind::StampRenewed,
                input.statement.digest().as_str().to_owned(),
            )
        }
    };
    change_records::insert(
        scope,
        &OrganizationChangeRecord {
            id: input.record_id,
            request_id: request_id.try_into()?,
            kind,
            origin: OrganizationChangeOrigin::SignInProvisioning {},
            target_id: organization.as_str().to_owned(),
            created_at_millis: now_millis,
        },
    )
    .await?;
    #[cfg(feature = "conformance")]
    if input.fail_after_record {
        return Err(StoreError::Aborted {
            reason: "conformance fault after the provisioning record".into(),
        });
    }
    Ok(ProvisioningOutcome {
        applied: ProvisioningApplied::Provisioned,
        refusal: None,
    })
}

/// The stored stamp's own `exp` is still ahead of `now`. Its signature and
/// key were checked when it was stored and by `stored_trusted`; this reads
/// only its expiry, without I/O, inside the applying transaction.
fn stored_unexpired_at(stored: &StoredStampRead, now: runtime_primitives::SyncTime) -> bool {
    let StoredStampRead::Stamped(stamp) = stored else {
        return false;
    };
    ParsedFleetStatement::parse(stamp.statement())
        .is_ok_and(|parsed| now.unix_millis().div_euclid(1000) < parsed.claims.exp.0)
}

fn unchanged(applied: ProvisioningApplied, _stored: StoredStampRead) -> ProvisioningOutcome {
    ProvisioningOutcome {
        applied,
        refusal: None,
    }
}

#[cfg(test)]
#[path = "provisioning_test.rs"]
mod tests;
