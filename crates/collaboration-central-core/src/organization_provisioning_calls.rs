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
            Err(
                FleetOrganizationStatementRefusal::VerificationUnavailable(_)
                | FleetOrganizationStatementRefusal::InvalidConfiguration,
            ) => {
                return Err(CallFailure::NotSent {
                    reason: NotSentReason::Overloaded,
                });
            }
            Err(FleetOrganizationStatementRefusal::OrganizationMismatch) => {
                return encode(ProvisioningApplied::OrganizationMismatch);
            }
            Err(_) => return encode(ProvisioningApplied::InvalidStatement),
        };
        let (outcome, _) = StepDriver::new(host.clone())
            .run(ProvisionStep(ApplyProvisioning {
                statement,
                compact: request.organization_statement.as_str().to_owned(),
                record_id: io.new_id(),
                now_millis: io.now().unix_millis(),
                stored_trusted,
                #[cfg(feature = "conformance")]
                fail_after_record: false,
            }))
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
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
