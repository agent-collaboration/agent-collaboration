//! The Fleet object's side of the private sign-in link: provisioning and the
//! renewal digest read, plus the stamp check every other command waits on.
use crate::{
    CallerKind, FleetKind, ObjectHost, ObjectRuntime, PlatformIo,
    caller_context::CallAuthority,
    fleet_organization_authority::StampStanding,
    identity_contract::ProvisionOrganization,
    object_call_envelope::ObjectCallEnvelope,
    object_runtime::ObjectWork,
    organization_authority::{
        ApplyProvisioning, FleetOrganizationStatementRefusal, ProvisioningApplied, StoredStampRead,
        apply_provisioning, read_stored_stamp,
    },
    step_driver::{StepCallback, StepDriver, StepResult},
};
use durable_steps::{
    AtomicStore, BoxFuture, CallFailure, CallTarget, NotSentReason, storage::TransactionScope,
};
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

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
        Type<Host::Database> + Decode<'value, Host::Database> + Encode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    /// Check the stored stamp once per process, before any command relies on it.
    pub(crate) async fn stamp_standing(&self) -> Result<StampStanding, CallFailure> {
        let ObjectWork::Fleet {
            organization_stamp,
            host,
            io,
            ..
        } = &self.work
        else {
            return Err(refused());
        };
        if organization_stamp.needs_check() {
            let (stored, _) = StepDriver::new(host.clone())
                .run(ReadStamp)
                .await
                .map_err(|_| CallFailure::AnswerLost)?;
            let verified = match &stored {
                StoredStampRead::Unprovisioned => None,
                StoredStampRead::Stamped(stamp) => {
                    let check = organization_stamp.check().ok_or_else(refused)?;
                    Some(check.verify_statement(stamp.statement()).await)
                }
            };
            organization_stamp.record(
                &stored,
                verified
                    .as_ref()
                    .map(|result| result.as_ref().map_err(|refusal| *refusal)),
            );
        }
        Ok(organization_stamp.standing(io.now()))
    }

    pub(crate) async fn provisioning_call(
        &self,
        call: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        require_sign_in_link(&call)?;
        let _: FleetProvisionOrganization =
            serde_json::from_value(call.target).map_err(|_| refused())?;
        let request: ProvisionOrganization =
            serde_json::from_value(call.request).map_err(|_| refused())?;
        let ObjectWork::Fleet {
            organization,
            organization_stamp,
            host,
            io,
            ..
        } = &self.work
        else {
            return Err(refused());
        };
        if &request.organization_id != organization {
            return encode(ProvisioningApplied::OrganizationMismatch);
        }
        let stored_trusted = self.stamp_standing().await? != StampStanding::Refused;
        let check = organization_stamp.check().ok_or_else(refused)?;
        // Signature and profile first, outside the transaction; nothing is
        // written for a statement that does not verify.
        let statement = match check
            .verify_statement(request.organization_statement.as_str())
            .await
        {
            Ok(statement) => statement,
            Err(refusal) => {
                observe_refusal(io.as_ref(), organization, refusal);
                return match refusal {
                    FleetOrganizationStatementRefusal::VerificationUnavailable(_)
                    | FleetOrganizationStatementRefusal::InvalidConfiguration => {
                        Err(CallFailure::NotSent {
                            reason: NotSentReason::Overloaded,
                        })
                    }
                    FleetOrganizationStatementRefusal::OrganizationMismatch => {
                        encode(ProvisioningApplied::OrganizationMismatch)
                    }
                    _ => encode(ProvisioningApplied::InvalidStatement),
                };
            }
        };
        // Conformance only: hold a verified statement here, before its
        // transaction, so another provisioning can commit in between.
        #[cfg(feature = "conformance")]
        io.before_organization_apply(FleetProvisionOrganization::KIND)
            .await?;
        let (outcome, _) = StepDriver::new(host.clone())
            .run(ProvisionStep(ApplyProvisioning {
                statement,
                compact: request.organization_statement.as_str().to_owned(),
                record_id: io.new_id(),
                clock: io.clone(),
                stored_trusted,
                #[cfg(feature = "conformance")]
                fail_after_record: io.take_organization_record_fault(),
            }))
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        if let Some(refusal) = outcome.refusal {
            observe_refusal(io.as_ref(), organization, refusal);
        }
        // The committed stamp is the one this process now trusts.
        if outcome.applied == ProvisioningApplied::Provisioned {
            organization_stamp.forget();
        }
        encode(outcome.applied)
    }

    pub(crate) async fn statement_digest_call(
        &self,
        call: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        require_sign_in_link(&call)?;
        let _: FleetOrganizationStatementDigest =
            serde_json::from_value(call.target).map_err(|_| refused())?;
        let (): () = serde_json::from_value(call.request).map_err(|_| refused())?;
        match self.stamp_standing().await? {
            StampStanding::Unprovisioned => encode(StatementDigestAnswer::Unprovisioned),
            StampStanding::Refused => Err(refused()),
            StampStanding::Valid => {
                let ObjectWork::Fleet {
                    host,
                    organization_stamp,
                    ..
                } = &self.work
                else {
                    return Err(refused());
                };
                let (stored, _) = StepDriver::new(host.clone())
                    .run(ReadStamp)
                    .await
                    .map_err(|_| CallFailure::AnswerLost)?;
                let StoredStampRead::Stamped(stamp) = stored else {
                    return encode(StatementDigestAnswer::Unprovisioned);
                };
                // Report only what the signature check establishes now; an
                // expired or unverifiable statement can never be described.
                let check = organization_stamp.check().ok_or_else(refused)?;
                let verified = check
                    .verify_statement(stamp.statement())
                    .await
                    .map_err(|_| refused())?;
                encode(StatementDigestAnswer::Current {
                    digest: verified.digest().as_str().to_owned(),
                    expires_at: verified.expires_at().0,
                    signing_key_id: verified.key_id().to_owned(),
                })
            }
        }
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
        Type<Host::Database> + Decode<'value, Host::Database> + Encode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    /// Floors only reach a Fleet whose stamp is valid; otherwise the push stays
    /// pending at the sign-in service.
    pub(crate) async fn authorization_revision_call(
        &self,
        call: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        require_sign_in_link(&call)?;
        let _: FleetAuthorizationRevision =
            serde_json::from_value(call.target).map_err(|_| refused())?;
        let revision: crate::identity_contract::AuthorizationRevision =
            serde_json::from_value(call.request).map_err(|_| refused())?;
        if self.stamp_standing().await? != StampStanding::Valid {
            return Err(refused());
        }
        let ObjectWork::Fleet {
            organization,
            host,
            io,
            ..
        } = &self.work
        else {
            return Err(refused());
        };
        if revision.organization_id() != organization {
            return encode(FloorDeliveryAnswer::OrganizationMismatch);
        }
        let (answer, _) = StepDriver::new(host.clone())
            .run(DeliverFloor {
                revision,
                now_ms: io.now().unix_millis(),
                #[cfg(feature = "conformance")]
                fail_after_write: io.take_organization_record_fault(),
            })
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        encode(answer)
    }
}

struct DeliverFloor {
    revision: crate::identity_contract::AuthorizationRevision,
    now_ms: i64,
    #[cfg(feature = "conformance")]
    fail_after_write: bool,
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for DeliverFloor
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Decode<'value, Driver> + Encode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = FloorDeliveryAnswer;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let delivered =
                crate::person_request::deliver_floor(scope, &self.revision, self.now_ms).await?;
            #[cfg(feature = "conformance")]
            if self.fail_after_write {
                return Err(durable_steps::StoreError::Aborted {
                    reason: "conformance fault after the floor write".into(),
                });
            }
            // The acknowledgement is the floor this same transaction retains.
            let answer = match delivered {
                crate::person_request::FloorDelivery::Acknowledged(revision) => {
                    FloorDeliveryAnswer::Acknowledged { revision }
                }
                crate::person_request::FloorDelivery::Conflict => FloorDeliveryAnswer::Conflict,
                crate::person_request::FloorDelivery::OrganizationFloorMissing => {
                    FloorDeliveryAnswer::OrganizationFloorMissing
                }
            };
            Ok((answer, Vec::new()))
        })
    }
}

fn require_sign_in_link(call: &ObjectCallEnvelope) -> Result<(), CallFailure> {
    if call.source_face != CallerKind::Edge
        || !matches!(call.authority, CallAuthority::SignInLink)
        || !call.has_fleet_source()
    {
        return Err(refused());
    }
    Ok(())
}

struct ReadStamp;
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for ReadStamp
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> Option<String>: Type<Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = StoredStampRead;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move { Ok((read_stored_stamp(scope).await?, Vec::new())) })
    }
}

struct ProvisionStep(ApplyProvisioning);
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for ProvisionStep
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Decode<'value, Driver> + Encode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = crate::organization_authority::ProvisioningOutcome;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move { Ok((apply_provisioning(scope, self.0).await?, Vec::new())) })
    }
}

fn encode<T: Serialize>(answer: T) -> Result<Vec<u8>, CallFailure> {
    serde_json::to_vec(&answer).map_err(|_| CallFailure::AnswerLost)
}

fn refused() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}

/// Hand one statement refusal to the host's telemetry: its closed kind and the
/// organization this Fleet serves, nothing from the statement itself.
fn observe_refusal(
    io: &impl crate::PlatformIo,
    organization: &fleet_identity_protocol::OrganizationId,
    refusal: FleetOrganizationStatementRefusal,
) {
    io.observe(crate::CentralObservation::OrganizationStatementRefused {
        organization: organization.clone(),
        refusal,
    });
}
