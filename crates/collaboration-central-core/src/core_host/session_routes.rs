//! Session callers can reach only these explicit DAG edges.
use super::{
    CallFailure, CallId, CallTarget, CallerContext, CopyKind, CopyRead, DeliveryKind, FleetKind,
    ReceiverId, RegistryKind, SerializedObjectCall, SessionKind, decode, encode_shared_call,
    no_route,
};

pub(super) fn classify_session(
    kind: &str,
    caller: &CallerContext<SessionKind>,
    authoritative_receiver: ReceiverId,
    target: serde_json::Value,
    request: serde_json::Value,
    identity: CallId,
) -> Result<SerializedObjectCall, CallFailure> {
    match kind {
        crate::mail_sender_calls::SenderFleetMail::KIND => {
            encode_shared_call::<SessionKind, FleetKind, crate::mail_sender_calls::SenderFleetMail>(
                caller,
                caller.fleet_address(),
                decode(target)?,
                decode(request)?,
                identity,
            )
        }
        crate::forward_calls::DeliverForward::KIND => {
            let target: crate::forward_calls::DeliverForward = decode(target)?;
            encode_shared_call::<SessionKind, DeliveryKind, crate::forward_calls::DeliverForward>(
                caller,
                caller.fleet_address(),
                target,
                decode(request)?,
                identity,
            )
        }
        crate::forward_calls::ForwardRoute::KIND => {
            encode_shared_call::<SessionKind, RegistryKind, crate::forward_calls::ForwardRoute>(
                caller,
                caller.fleet_address(),
                decode(target)?,
                decode(request)?,
                identity,
            )
        }
        crate::forward_calls::InstallationPeerRegistry::KIND => encode_shared_call::<
            SessionKind,
            RegistryKind,
            crate::forward_calls::InstallationPeerRegistry,
        >(
            caller,
            caller.fleet_address(),
            decode(target)?,
            decode(request)?,
            identity,
        ),
        crate::installation_peer_reads::InstallationPeerDirectory::KIND => encode_shared_call::<
            SessionKind,
            RegistryKind,
            crate::installation_peer_reads::InstallationPeerDirectory,
        >(
            caller,
            caller.fleet_address(),
            decode(target)?,
            decode(request)?,
            identity,
        ),
        crate::fleet_read_calls::FleetRead::KIND => {
            encode_shared_call::<SessionKind, FleetKind, crate::fleet_read_calls::FleetRead>(
                caller,
                caller.fleet_address(),
                decode(target)?,
                decode(request)?,
                identity,
            )
        }
        crate::mail_submit_calls::FleetMailSubmit::KIND => {
            encode_shared_call::<SessionKind, FleetKind, crate::mail_submit_calls::FleetMailSubmit>(
                caller,
                caller.fleet_address(),
                decode(target)?,
                decode(request)?,
                identity,
            )
        }
        fleet_sync_session::SchemaSupportRegistry::KIND => {
            encode_shared_call::<SessionKind, RegistryKind, fleet_sync_session::SchemaSupportRegistry>(
                caller,
                caller.fleet_address(),
                decode(target)?,
                decode(request)?,
                identity,
            )
        }
        crate::session_calls::WelcomeLeases::KIND => {
            encode_shared_call::<SessionKind, RegistryKind, crate::session_calls::WelcomeLeases>(
                caller,
                caller.fleet_address(),
                decode(target)?,
                decode(request)?,
                identity,
            )
        }
        crate::session_calls::SessionLeaseRegistry::KIND => encode_shared_call::<
            SessionKind,
            RegistryKind,
            crate::session_calls::SessionLeaseRegistry,
        >(
            caller,
            caller.fleet_address(),
            decode(target)?,
            decode(request)?,
            identity,
        ),
        crate::fleet_runtime::FleetRequestEffects::KIND => {
            encode_shared_call::<SessionKind, FleetKind, crate::fleet_runtime::FleetRequestEffects>(
                caller,
                caller.fleet_address(),
                decode(target)?,
                decode(request)?,
                identity,
            )
        }
        crate::fleet_metadata_calls::FleetMetadata::KIND => {
            encode_shared_call::<SessionKind, FleetKind, crate::fleet_metadata_calls::FleetMetadata>(
                caller,
                caller.fleet_address(),
                decode(target)?,
                decode(request)?,
                identity,
            )
        }
        crate::fleet_archive_calls::FleetArchiveReport::KIND => encode_shared_call::<
            SessionKind,
            FleetKind,
            crate::fleet_archive_calls::FleetArchiveReport,
        >(
            caller,
            caller.fleet_address(),
            decode(target)?,
            decode(request)?,
            identity,
        ),
        CopyRead::KIND => {
            let target: CopyRead = decode(target)?;
            if target.copy != authoritative_receiver {
                return Err(no_route());
            }
            encode_shared_call::<SessionKind, CopyKind, CopyRead>(
                caller,
                caller.copy_address(target.database),
                target,
                decode(request)?,
                identity,
            )
        }
        crate::copy_write_calls::CopySessionWrite::KIND => {
            let target: crate::copy_write_calls::CopySessionWrite = decode(target)?;
            if target.copy != authoritative_receiver {
                return Err(no_route());
            }
            encode_shared_call::<SessionKind, CopyKind, crate::copy_write_calls::CopySessionWrite>(
                caller,
                caller.copy_address(target.database),
                target,
                decode(request)?,
                identity,
            )
        }
        crate::copy_read_calls::CopySessionRead::KIND => {
            let target: crate::copy_read_calls::CopySessionRead = decode(target)?;
            if target.copy != authoritative_receiver {
                return Err(no_route());
            }
            encode_shared_call::<SessionKind, CopyKind, crate::copy_read_calls::CopySessionRead>(
                caller,
                caller.copy_address(target.database),
                target,
                decode(request)?,
                identity,
            )
        }
        _ => Err(no_route()),
    }
}
