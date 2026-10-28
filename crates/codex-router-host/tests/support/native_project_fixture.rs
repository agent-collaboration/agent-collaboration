//! Explicit fixture enrollment and unsettled directory registration for native local tests.
use board_fleet_sync::{AccessServiceToken, BoardSessionInputs};
use collaboration_contracts::ProjectRegistration;
use collaboration_contracts::ProjectRegistrationFuture;
use collaboration_contracts::ProjectRegistrationState;
use collaboration_runtime::ProjectStoreRegistry;
use fleet_sync_protocol::{PeerId, ReceiverId};
use fleet_sync_session::{Endpoint, PeerCredential};
use runtime_primitives::SystemClock;
use std::{io, net::SocketAddr, path::Path, sync::Arc};

pub struct UnsettledRegistration;
impl ProjectRegistration for UnsettledRegistration {
    fn wait_ready<'runtime>(
        &'runtime self,
        _: &'runtime message_board::ProjectId,
    ) -> ProjectRegistrationFuture<'runtime> {
        Box::pin(async { Ok(ProjectRegistrationState::Pending) })
    }

    fn start<'runtime>(
        &'runtime self,
        _: &'runtime message_board::ProjectId,
    ) -> ProjectRegistrationFuture<'runtime> {
        Box::pin(async { Ok(ProjectRegistrationState::Pending) })
    }
}

pub fn registry(directory: &Path) -> io::Result<Arc<ProjectStoreRegistry>> {
    Ok(Arc::new(ProjectStoreRegistry::new(
        directory,
        Some(collaboration_runtime::load_installation_peer(directory)?),
        Arc::new(SystemClock),
    )))
}

/// The caller retains its own listening socket without sending Welcome. This is
/// declared offline fixture enrollment, not a central or authentication observation.
pub fn offline_inputs(address: SocketAddr, peer: PeerId) -> io::Result<BoardSessionInputs> {
    Ok(BoardSessionInputs {
        endpoint: Endpoint::new(&format!("ws://{address}/session")).map_err(io::Error::other)?,
        access: AccessServiceToken::new("fixture-access", "fixture-secret")
            .map_err(io::Error::other)?,
        credential: PeerCredential::new("Bearer native-local-fixture").map_err(io::Error::other)?,
        peer,
        receiver: ReceiverId::try_from("0192a1b2-c3d4-7000-8000-000000000002".to_owned())
            .map_err(io::Error::other)?,
        // Named offline fixture configuration; it is independent of the installation peer.
        relay: "0192a1b2-c3d4-7000-8000-000000000003"
            .parse()
            .map_err(io::Error::other)?,
    })
}
