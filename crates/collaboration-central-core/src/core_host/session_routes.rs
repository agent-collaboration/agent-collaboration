//! Session callers can reach only these explicit DAG edges.
use super::{
    CallFailure, CallId, CallTarget, CallerContext, RegistryKind, SerializedObjectCall,
    SessionKind, decode, encode_shared_call, no_route,
};

pub(super) fn classify_session(
    kind: &str,
    caller: &CallerContext<SessionKind>,
    target: serde_json::Value,
    request: serde_json::Value,
    identity: CallId,
) -> Result<SerializedObjectCall, CallFailure> {
    match kind {
        fleet_sync_session::SchemaSupportRegistry::KIND => {
            encode_shared_call::<SessionKind, RegistryKind, fleet_sync_session::SchemaSupportRegistry>(
                caller,
                (),
                decode(target)?,
                decode(request)?,
                identity,
            )
        }
        _ => Err(no_route()),
    }
}
