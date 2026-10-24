//! Directory reads leave the connection's local section before entering Registry.
use super::EffectDriver;
use crate::{
    ObjectHost, PlatformIo,
    forward_calls::InstallationPeerRegistry,
    installation_peer_reads::{
        InstallationPeerDirectory, InstallationPeerPageRead, InstallationPeerPageRequest,
        InstallationReadRequest,
    },
};
use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerProtocol, BoardReadPage, InstallationPeerLookup,
};
use durable_steps::{CallFailure, Host as _};
use fleet_sync_protocol::{ConnectionId, FrameId, OperationAnswer, ServerFrame};
use fleet_sync_session::SessionKey;

impl<Host: ObjectHost, Io: PlatformIo> EffectDriver<Host, Io> {
    pub(super) async fn answer_installation_read(
        &self,
        connection: ConnectionId,
        session: SessionKey,
        frame: FrameId,
        request: InstallationReadRequest,
    ) {
        if !self.is_attached(connection, session.session_id).await {
            return;
        }
        let answer: Result<OperationAnswer<BoardConsumerProtocol>, CallFailure> = async {
            let page = match request {
                InstallationReadRequest::Lookup(installation) => {
                    let peer = self
                        .shared_host
                        .dispatch(
                            InstallationPeerRegistry { installation },
                            (),
                            self.shared_host.call_identity("installation_peer")?,
                        )
                        .await?;
                    BoardReadPage::InstallationPeer(match peer {
                        Some(peer) => InstallationPeerLookup::Enrolled { peer },
                        None => InstallationPeerLookup::Unknown,
                    })
                }
                InstallationReadRequest::Page(after) => {
                    match self
                        .shared_host
                        .dispatch(
                            InstallationPeerDirectory,
                            InstallationPeerPageRequest { after },
                            self.shared_host.call_identity("installation_peers")?,
                        )
                        .await?
                    {
                        InstallationPeerPageRead::Page(page) => {
                            BoardReadPage::InstallationPeers(page)
                        }
                        InstallationPeerPageRead::CursorInvalid => {
                            return Ok(OperationAnswer::InvalidShape {
                                field: "after".into(),
                            });
                        }
                    }
                }
            };
            Ok(OperationAnswer::Consumer(BoardConsumerAnswer::Read(page)))
        }
        .await;
        if !self.is_attached(connection, session.session_id).await {
            return;
        }
        let Ok(body) = answer else {
            self.fail_connection(connection).await;
            return;
        };
        match serde_json::to_vec(&ServerFrame::<BoardConsumerProtocol>::Answer { frame, body }) {
            Ok(bytes) => {
                if self
                    .host
                    .send_frame(connection, bytes.into())
                    .await
                    .is_err()
                {
                    self.fail_connection(connection).await;
                }
            }
            Err(_) => self.fail_connection(connection).await,
        }
    }
}
