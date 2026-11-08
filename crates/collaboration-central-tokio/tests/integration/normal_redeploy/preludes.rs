//! The original transport, renewal and zero-window bodies share only socket setup.
use super::*;
#[path = "renewal_window.rs"]
mod renewal_window;
#[path = "transport_preludes.rs"]
mod transport_preludes;

struct NormalPeer {
    installation: message_board::ServiceId,
    peer: PeerId,
    credential: String,
    session: SessionId,
    socket: WebSocket,
}
impl NormalPeer {
    async fn enroll(base: &str, label: &str) -> ConformanceResult<Self> {
        let client = ConformanceClient::new(base)?;
        let installation = message_board::ServiceId::try_from(Uuid::now_v7().to_string())?;
        let peer = PeerId::new(Uuid::now_v7())?;
        let credential = format!("row10 {label} {}", Uuid::now_v7());
        let session = SessionId::new(Uuid::now_v7())?;
        let admitted = collaboration_central_conformance::enroll_admitted_peer_with_label(
            &client,
            installation.as_str(),
            &peer.to_string(),
            &credential,
            label,
            &session.to_string(),
        )
        .await?;
        Ok(Self {
            installation,
            peer,
            credential,
            session,
            socket: admitted.socket,
        })
    }
    async fn close(self) -> ConformanceResult<()> {
        self.socket
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await?;
        Ok(())
    }
}
fn http() -> ConformanceResult<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?)
}
async fn original_rows(socket: &mut WebSocket, database: DatabaseId) -> ConformanceResult<()> {
    transport::exact(
        socket,
        database,
        position(1, 3)?,
        &[(1, "imported-1"), (2, "imported-2"), (3, "imported-3")],
    )
    .await
}
async fn next_operation(
    socket: &mut WebSocket,
) -> ConformanceResult<ServerFrame<BoardConsumerProtocol>> {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let frame = transport::next(socket).await?;
            if !matches!(frame, ServerFrame::Notice { .. }) {
                return Ok(frame);
            }
        }
    })
    .await?
}
pub(in super::super) async fn run(
    base: &str,
    platform: &collaboration_central_tokio::TokioCentralPlatform,
    [moving, forwarding, owner_loss, renewal, window]: [DatabaseId; 5],
) -> ConformanceResult<()> {
    creation::pin_registry_clock(base, RegistryTime("2030-03-17T17:46:40Z".parse()?)).await?;
    transport_preludes::clean_move(base, moving).await?;
    transport_preludes::forward_roundtrip(base, forwarding).await?;
    transport_preludes::owner_response_loss(base, owner_loss).await?;
    renewal_window::renewal_replay(base, renewal).await?;
    renewal_window::same_socket_zero_window(base, platform, window).await?;
    println!(
        "NORMAL_REDEPLOY_STAGE all_original_transport_renewal_and_zero_window_preludes passed"
    );
    Ok(())
}
