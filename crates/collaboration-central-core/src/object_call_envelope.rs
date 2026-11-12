//! One source envelope for direct calls and the engine's registered targets.

use crate::{
    CallerContext, CallerKind, MayCall, ObjectAddress, ObjectKind, SerializedObjectCall,
    caller_context::CallAuthority,
};
use durable_steps::{CallFailure, CallId, CallTarget, NotSentReason};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ObjectCallEnvelope {
    pub source: ObjectAddress,
    pub source_face: CallerKind,
    pub authority: CallAuthority,
    pub destination: ObjectAddress,
    pub destination_face: CallerKind,
    pub identity: CallId,
    pub kind: String,
    // CallTarget is an open generic boundary. The receiving core classifies
    // these fields into the registered upstream target and request together.
    pub target: serde_json::Value,
    pub request: serde_json::Value,
}

pub(crate) fn encode_shared_call<From, To, Call>(
    caller: &CallerContext<From>,
    destination: To::Address,
    target: Call,
    request: Call::Request,
    identity: CallId,
) -> Result<SerializedObjectCall, CallFailure>
where
    From: ObjectKind + MayCall<To>,
    To: ObjectKind,
    Call: CallTarget,
{
    let destination = To::address(&destination);
    let envelope = ObjectCallEnvelope {
        source: caller.address(),
        source_face: From::KIND,
        authority: caller.authority.clone(),
        destination,
        destination_face: To::KIND,
        identity,
        kind: Call::KIND.into(),
        target: serde_json::to_value(target).map_err(encode_refusal)?,
        request: serde_json::to_value(request).map_err(encode_refusal)?,
    };
    encode_envelope(envelope)
}

pub(crate) fn encode_refusal(_error: impl std::fmt::Display) -> CallFailure {
    // The shared Host contract has no encoding-error case. Like its native
    // host, refuse before dispatch; never turn it into an unknown effect.
    CallFailure::NotSent {
        reason: NotSentReason::Overloaded,
    }
}

pub(crate) fn encode_envelope(
    envelope: ObjectCallEnvelope,
) -> Result<SerializedObjectCall, CallFailure> {
    let destination = envelope.destination.clone();
    let bytes = serde_json::to_vec(&envelope).map_err(encode_refusal)?;
    Ok(SerializedObjectCall { destination, bytes })
}
