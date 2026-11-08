//! The Fleet object's adapter for the private sign-in link. It checks each
//! call's envelope, hands the link's handlers the narrow view they need
//! ([`OrganizationLinkFleet`]), runs their store steps and encodes the answers.
//! The decisions live in `organization_provisioning_calls`.
use crate::{
    CallerKind, CentralObservation, FleetKind, ObjectHost, ObjectRuntime, PlatformIo,
    caller_context::CallAuthority,
    fleet_organization_authority::{FleetOrganizationAuthority, StampStanding},
    identity_contract::{AuthorizationRevision, ProvisionOrganization},
    object_call_envelope::ObjectCallEnvelope,
    object_runtime::ObjectWork,
    organization_authority::{
        ApplyProvisioning, ProvisioningOutcome, StoredStampRead, apply_provisioning,
        read_stored_stamp,
    },
    organization_provisioning_calls::{
        self as link, FleetAuthorizationRevision, FleetOrganizationStatementDigest,
        FleetProvisionOrganization, FloorDeliveryAnswer, OrganizationLinkFleet, refused,
    },
    step_driver::{StepCallback, StepDriver, StepResult},
};
use durable_steps::{AtomicStore, BoxFuture, CallFailure, Uuid, storage::TransactionScope};
use fleet_identity_protocol::OrganizationId;
use runtime_primitives::{Clock, SyncTime};
use serde::Serialize;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::{convert::Infallible, sync::Arc};

/// The parts of a Fleet object the link's handlers act through.
struct FleetLink<'fleet, Host, Io> {
    organization: &'fleet OrganizationId,
    authority: &'fleet FleetOrganizationAuthority,
    host: &'fleet Arc<Host>,
    io: &'fleet Arc<Io>,
}

impl<Host: ObjectHost, Io: PlatformIo> OrganizationLinkFleet for FleetLink<'_, Host, Io>
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
    fn organization(&self) -> &OrganizationId {
        self.organization
    }
    fn stamp_authority(&self) -> &FleetOrganizationAuthority {
        self.authority
    }
    fn now(&self) -> SyncTime {
        self.io.now()
    }
    fn new_id(&self) -> Uuid {
        self.io.new_id()
    }
    fn clock(&self) -> Arc<dyn Clock> {
        self.io.clone()
    }
    fn observe(&self, observation: CentralObservation) {
        self.io.observe(observation);
    }
    async fn read_stamp(&self) -> Result<StoredStampRead, CallFailure> {
        let (stored, _) = StepDriver::new(self.host.clone())
            .run(ReadStamp)
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        Ok(stored)
    }
    async fn apply_provisioning(
        &self,
        input: ApplyProvisioning,
    ) -> Result<ProvisioningOutcome, CallFailure> {
        let (outcome, _) = StepDriver::new(self.host.clone())
            .run(ProvisionStep(input))
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        Ok(outcome)
    }
    async fn deliver_floor(
        &self,
        revision: AuthorizationRevision,
        now_ms: i64,
    ) -> Result<FloorDeliveryAnswer, CallFailure> {
        let (answer, _) = StepDriver::new(self.host.clone())
            .run(DeliverFloor {
                revision,
                now_ms,
                #[cfg(feature = "conformance")]
                fail_after_write: self.io.take_organization_record_fault(),
            })
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        Ok(answer)
    }
    #[cfg(feature = "conformance")]
    async fn before_apply(&self, kind: &'static str) -> Result<(), CallFailure> {
        self.io.before_organization_apply(kind).await
    }
    #[cfg(feature = "conformance")]
    fn take_record_fault(&self) -> bool {
        self.io.take_organization_record_fault()
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
    fn organization_link(&self) -> Result<FleetLink<'_, Host, Io>, CallFailure> {
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
        Ok(FleetLink {
            organization,
            authority: organization_stamp,
            host,
            io,
        })
    }

    /// Check the stored stamp once per process, before any command relies on it.
    pub(crate) async fn stamp_standing(&self) -> Result<StampStanding, CallFailure> {
        link::stamp_standing(&self.organization_link()?).await
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
        encode(link::provision(&self.organization_link()?, request).await?)
    }

    pub(crate) async fn statement_digest_call(
        &self,
        call: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        require_sign_in_link(&call)?;
        let _: FleetOrganizationStatementDigest =
            serde_json::from_value(call.target).map_err(|_| refused())?;
        let (): () = serde_json::from_value(call.request).map_err(|_| refused())?;
        encode(link::statement_digest(&self.organization_link()?).await?)
    }

    /// Floors only reach a Fleet whose stamp is valid; otherwise the push stays
    /// pending at the sign-in service.
    pub(crate) async fn authorization_revision_call(
        &self,
        call: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        require_sign_in_link(&call)?;
        let _: FleetAuthorizationRevision =
            serde_json::from_value(call.target).map_err(|_| refused())?;
        let revision: AuthorizationRevision =
            serde_json::from_value(call.request).map_err(|_| refused())?;
        encode(link::authorization_revision(&self.organization_link()?, revision).await?)
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

fn encode<T: Serialize>(answer: T) -> Result<Vec<u8>, CallFailure> {
    serde_json::to_vec(&answer).map_err(|_| CallFailure::AnswerLost)
}

struct DeliverFloor {
    revision: AuthorizationRevision,
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
    type Output = ProvisioningOutcome;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move { Ok((apply_provisioning(scope, self.0).await?, Vec::new())) })
    }
}
