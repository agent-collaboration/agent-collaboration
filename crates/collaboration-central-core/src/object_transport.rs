//! The core binds call authority to its source before a runtime transports bytes.
use crate::FleetKind;

use crate::object_call_envelope::{ObjectCallEnvelope, encode_envelope, encode_refusal};
use crate::{
    CallerContext, CallerKind, Dispatch, MayCall, ObjectAddress, ObjectHost, ObjectKind,
    ObjectRuntime, PlatformIo, RegistryCall, RegistryResult, WireCall, WireKind,
    caller_context::CallAuthority,
};
use durable_steps::{
    AtomicStore, CallFailure, CallTarget, NotSentReason, storage::TransactionScope,
};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub struct SerializedObjectCall {
    pub(crate) destination: ObjectAddress,
    pub(crate) bytes: Vec<u8>,
}
impl SerializedObjectCall {
    pub fn destination(&self) -> &ObjectAddress {
        &self.destination
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn into_parts(self) -> (ObjectAddress, Vec<u8>) {
        (self.destination, self.bytes)
    }
}

pub fn serialize_object_call<From, To>(
    caller: &CallerContext<From>,
    call: Dispatch<To>,
) -> Result<SerializedObjectCall, CallFailure>
where
    From: ObjectKind + MayCall<To>,
    To: WireKind,
{
    if caller.organization() != To::organization(&call.to) {
        return Err(refused());
    }
    let identity = durable_steps::CallId {
        operation: durable_steps::OperationId::new(
            durable_steps::Uuid::parse_str(call.call_id.as_str()).map_err(encode_refusal)?,
        )
        .map_err(encode_refusal)?,
        step: durable_steps::StepName::new("object").map_err(encode_refusal)?,
    };
    encode_envelope(ObjectCallEnvelope {
        organization: caller.organization().clone(),
        source: caller.address(),
        source_face: From::KIND,
        authority: caller.authority.clone(),
        destination: To::address(&call.to),
        destination_face: To::KIND,
        identity,
        kind: "central.object".into(),
        target: serde_json::Value::Null,
        request: serde_json::to_value(To::call(call.call)).map_err(encode_refusal)?,
    })
}

pub fn deserialize_object_reply<To: ObjectKind>(bytes: &[u8]) -> Result<To::Result, CallFailure> {
    serde_json::from_slice(bytes).map_err(|_| CallFailure::AnswerLost)
}

impl ObjectCallEnvelope {
    pub(crate) fn decode(
        bytes: &[u8],
        destination: ObjectAddress,
        face: CallerKind,
        organization: &fleet_identity_protocol::OrganizationId,
    ) -> Result<Self, CallFailure> {
        let call: Self = serde_json::from_slice(bytes).map_err(|_| refused())?;
        call.validate(destination, face, organization)
    }

    // Classification does not authorize a Copy operation. The private reference
    // is checked asynchronously before health reads or ordinary dispatch.
    pub(crate) fn decode_copy_candidate(
        bytes: &[u8],
        database: fleet_sync_protocol::DatabaseId,
    ) -> Result<Self, CallFailure> {
        let call: Self = serde_json::from_slice(bytes).map_err(|_| refused())?;
        let organization = call.organization.clone();
        call.validate(
            ObjectAddress::Copy(database),
            CallerKind::Copy,
            &organization,
        )
    }

    fn validate(
        self,
        destination: ObjectAddress,
        face: CallerKind,
        organization: &fleet_identity_protocol::OrganizationId,
    ) -> Result<Self, CallFailure> {
        let call = self;
        if &call.organization != organization
            || call.destination != destination
            || call.destination_face != face
            || !(call.source_face.may_call(face)
                || (call.source_face == CallerKind::Copy
                    && face == CallerKind::Fleet
                    && call.kind == crate::copy_directory_summary::DirectorySummaryUpdate::KIND
                    && matches!(call.authority, CallAuthority::Object)))
            || !source_matches(&call)
        {
            return Err(refused());
        }
        Ok(call)
    }
}
fn source_matches(call: &ObjectCallEnvelope) -> bool {
    let fleet = ObjectAddress::Fleet {
        organization: call.organization.clone(),
    };
    match &call.authority {
        CallAuthority::Owner(_) => {
            call.source == fleet && matches!(call.source_face, CallerKind::Edge | CallerKind::Fleet)
        }
        CallAuthority::AccessMachine(_) | CallAuthority::SignInLink => {
            call.source_face == CallerKind::Edge && call.source == fleet
        }
        CallAuthority::Object => {
            (call.source == fleet
                && matches!(
                    call.source_face,
                    CallerKind::Registry | CallerKind::Fleet | CallerKind::Delivery
                ))
                || matches!(
                    (&call.source, call.source_face),
                    (ObjectAddress::Copy(_), CallerKind::Copy)
                )
        }
        CallAuthority::Machine { .. } => {
            call.source == fleet && call.source_face == CallerKind::Session
        }
    }
}

fn refused() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}

impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<FleetKind, Host, Io>
where
    Host::Store: durable_steps::ReborrowScope,
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'blob> Vec<u8>:
        Type<Host::Database> + Encode<'blob, Host::Database> + Decode<'blob, Host::Database>,
    for<'blob> Option<Vec<u8>>: Type<Host::Database> + Decode<'blob, Host::Database>,
    for<'scalar> i64: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'scalar, Host::Database>
        + sqlx_core::decode::Decode<'scalar, Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub(crate) async fn on_registry_call(&self, bytes: Vec<u8>) -> Result<Vec<u8>, CallFailure> {
        let invocation = ObjectCallEnvelope::decode(
            &bytes,
            ObjectAddress::Fleet {
                organization: self.fleet_address()?.organization,
            },
            CallerKind::Registry,
            &self.fleet_address()?.organization,
        )?;
        if invocation.kind == crate::owner_registry_calls::OwnerRegistryOperations::KIND {
            return self.owner_registry_call(invocation).await;
        }
        if invocation.kind == crate::admin_import_registry::AdminImportReservation::KIND {
            return self.admin_import_reservation(invocation).await;
        }
        #[cfg(feature = "conformance")]
        if invocation.kind
            == crate::machine_enrollment::conformance_revocation::ControlRevocation::KIND
        {
            return self.control_revocation(invocation).await;
        }
        #[cfg(feature = "conformance")]
        if invocation.kind == crate::conformance_holder::HolderProbe::KIND {
            return self.holder_probe(invocation).await;
        }
        #[cfg(feature = "conformance")]
        if invocation.kind == crate::conformance_holder::HolderProbeStatus::KIND {
            return self.holder_probe_status(invocation).await;
        }
        #[cfg(feature = "conformance")]
        if invocation.kind == crate::conformance_holder::ObserveRegistryDispatches::KIND {
            return self.registry_dispatches(invocation);
        }
        #[cfg(feature = "conformance")]
        if invocation.kind == crate::conformance_copy::CopyProbe::KIND {
            return self.copy_probe(invocation).await;
        }
        #[cfg(feature = "conformance")]
        if invocation.kind == crate::conformance_copy::CopyProbeStatus::KIND {
            return self.copy_probe_status(invocation).await;
        }
        #[cfg(feature = "conformance")]
        if invocation.kind == crate::conformance_copy_wait::ControlCopyWait::KIND {
            return self.control_copy_wait(invocation).await;
        }
        #[cfg(feature = "conformance")]
        if invocation.kind == crate::conformance_copy::ObserveDriveTokens::KIND {
            return self.observe_drive_tokens(invocation);
        }
        if invocation.kind == crate::session_calls::SessionLeaseRegistry::KIND {
            return self.lease_call(invocation).await;
        }
        if matches!(
            invocation.kind.as_str(),
            crate::forward_calls::ForwardRoute::KIND
                | crate::forward_calls::InstallationPeerRegistry::KIND
                | crate::installation_peer_reads::InstallationPeerDirectory::KIND
        ) {
            return self.registry_machine_read(invocation).await;
        }
        if invocation.kind != "central.object" {
            return self.session_call(invocation).await;
        }
        if invocation.target != serde_json::Value::Null {
            return Err(refused());
        }
        let call: WireCall = serde_json::from_value(invocation.request).map_err(|_| refused())?;
        let WireCall::Registry(call) = call else {
            return Err(refused());
        };
        let result = match (invocation.authority, call) {
            (CallAuthority::Owner(owner), RegistryCall::RevokeEnrollment { installation }) => {
                RegistryResult::EnrollmentRevoked(
                    self.revoke_machine(&owner, installation)
                        .await
                        .map_err(|_| CallFailure::AnswerLost)?,
                )
            }
            (CallAuthority::Owner(owner), RegistryCall::EnrollMachine(request)) => {
                RegistryResult::Enrollment(
                    self.enroll_machine(&owner, request)
                        .await
                        .map_err(|_| CallFailure::AnswerLost)?,
                )
            }
            (
                CallAuthority::AccessMachine(client),
                RegistryCall::AuthenticateEnrollment { credential_hash },
            ) => RegistryResult::EnrollmentAuthenticated(
                self.authenticate_machine(client, credential_hash)
                    .await
                    .map_err(|_| CallFailure::AnswerLost)?,
            ),
            _ => return Err(refused()),
        };
        serde_json::to_vec(&result).map_err(|_| CallFailure::AnswerLost)
    }
}

#[cfg(test)]
#[path = "object_transport_test.rs"]
mod tests;
