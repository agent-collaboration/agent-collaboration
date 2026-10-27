//! The core binds call authority to its source before a runtime transports bytes.

use crate::object_call_envelope::{ObjectCallEnvelope, encode_envelope, encode_refusal};
use crate::{
    CallerContext, CallerKind, Dispatch, MayCall, ObjectAddress, ObjectHost, ObjectKind,
    ObjectRuntime, PlatformIo, RegistryCall, RegistryKind, RegistryResult, WireCall, WireKind,
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
    let identity = durable_steps::CallId {
        operation: durable_steps::OperationId::new(
            durable_steps::Uuid::parse_str(call.call_id.as_str()).map_err(encode_refusal)?,
        )
        .map_err(encode_refusal)?,
        step: durable_steps::StepName::new("object").map_err(encode_refusal)?,
    };
    encode_envelope(ObjectCallEnvelope {
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
    ) -> Result<Self, CallFailure> {
        let call: Self = serde_json::from_slice(bytes).map_err(|_| refused())?;
        if call.destination != destination
            || call.destination_face != face
            || !call.source_face.may_call(face)
            || !source_matches(&call)
        {
            return Err(refused());
        }
        Ok(call)
    }
}
fn source_matches(call: &ObjectCallEnvelope) -> bool {
    match &call.authority {
        CallAuthority::Owner(_) | CallAuthority::AccessMachine(_) => {
            call.source_face == CallerKind::Edge && call.source == ObjectAddress::Registry
        }
        CallAuthority::Object => matches!(
            (&call.source, call.source_face),
            (ObjectAddress::Registry, CallerKind::Registry)
                | (ObjectAddress::Fleet, CallerKind::Fleet)
                | (ObjectAddress::Copy(_), CallerKind::Copy)
                | (ObjectAddress::Mailbox(_), CallerKind::Mailbox)
                | (
                    ObjectAddress::Connection(_),
                    CallerKind::Session | CallerKind::Delivery
                )
        ),
    }
}
fn refused() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}

impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<RegistryKind, Host, Io>
where
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub async fn on_call(&self, bytes: Vec<u8>) -> Result<Vec<u8>, CallFailure> {
        let invocation =
            ObjectCallEnvelope::decode(&bytes, ObjectAddress::Registry, CallerKind::Registry)?;
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
        if invocation.kind == crate::mail_submit_calls::MailSubmitRegistry::KIND {
            return self.mail_submit_call(invocation).await;
        }
        if invocation.kind == crate::mail_sender_calls::SenderMailboxRegistry::KIND {
            return self.sender_mailbox_call(invocation).await;
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
mod tests {
    use super::*;
    use serde_json::json;

    fn object_envelope(
        source: serde_json::Value,
        source_face: &str,
        destination: &str,
    ) -> serde_json::Result<Vec<u8>> {
        serde_json::to_vec(&json!({
            "source": source,
            "sourceFace": source_face,
            "authority": {"kind": "object"},
            "destination": {"kind": destination},
            "destinationFace": destination,
            "identity": {"operation": "019f0000-0000-7000-8000-000000000806", "step": "object"},
            "kind": "central.installation-peer",
            "target": {"installation": "550e8400-e29b-41d4-a716-446655440806"},
            "request": null,
        }))
    }

    #[expect(
        clippy::panic_in_result_fn,
        reason = "Assertions must fail this test while fixture errors propagate through Result."
    )]
    #[test]
    fn decoder_refuses_the_well_formed_copy_to_registry_edge() -> serde_json::Result<()> {
        let bytes = object_envelope(
            json!({"kind": "copy", "value": "019f0000-0000-7000-8000-000000000806"}),
            "copy",
            "registry",
        )?;
        assert!(matches!(
            ObjectCallEnvelope::decode(&bytes, ObjectAddress::Registry, CallerKind::Registry),
            Err(CallFailure::NotSent {
                reason: NotSentReason::Unreachable { last_seen: None }
            })
        ));
        Ok(())
    }

    #[expect(
        clippy::panic_in_result_fn,
        reason = "Assertions must fail this test while fixture errors propagate through Result."
    )]
    #[test]
    fn decoder_allows_the_session_to_registry_edge() -> serde_json::Result<()> {
        let bytes = object_envelope(
            json!({"kind": "connection", "value": "550e8400-e29b-41d4-a716-446655440806"}),
            "session",
            "registry",
        )?;
        assert!(
            ObjectCallEnvelope::decode(&bytes, ObjectAddress::Registry, CallerKind::Registry)
                .is_ok()
        );
        Ok(())
    }

    #[expect(
        clippy::panic_in_result_fn,
        reason = "Assertions must fail this test while fixture errors propagate through Result."
    )]
    #[test]
    fn decoder_allows_the_copy_to_fleet_edge() -> serde_json::Result<()> {
        let bytes = object_envelope(
            json!({"kind": "copy", "value": "019f0000-0000-7000-8000-000000000806"}),
            "copy",
            "fleet",
        )?;
        assert!(
            ObjectCallEnvelope::decode(&bytes, ObjectAddress::Fleet, CallerKind::Fleet).is_ok()
        );
        Ok(())
    }
}
