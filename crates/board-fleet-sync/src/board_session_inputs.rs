//! Enrollment supplies connection inputs; the Host owns the one machine session.
use crate::AccessServiceToken;
use fleet_sync_protocol::{PeerId, ReceiverId};
use fleet_sync_session::{Endpoint, PeerCredential};

pub struct BoardSessionInputs {
    pub endpoint: Endpoint,
    pub access: AccessServiceToken,
    pub credential: PeerCredential,
    pub peer: PeerId,
    pub receiver: ReceiverId,
    /// The deployment's configured relay, returned by enrollment without derivation.
    pub relay: PeerId,
}
