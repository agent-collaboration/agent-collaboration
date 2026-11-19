use super::*;
use central_link_protocol::{DataFrame, FrameDirection, LinkFrame, MachineTokenText, encode_text};
use collaboration_runtime::{
    CentralLinkRefusal, CentralLinkStatus, ConfiguredCentralLink, LinkInbound, LinkSocket,
    LinkTransport, LinkTransportError, MachineTokenSource, TokenAvailability, TransportFuture,
};
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

struct FixtureTokens(MachineTokenText);

impl MachineTokenSource for FixtureTokens {
    fn availability(&self) -> TokenAvailability {
        TokenAvailability::Ready(self.0.clone())
    }

    fn changed(&self) -> TransportFuture<'_, ()> {
        Box::pin(std::future::pending())
    }

    fn report_renewal(&self, _: &central_link_protocol::TokenRenewalResult) {}
}

struct WelcomeTransport {
    expected_token: MachineTokenText,
    socket: Mutex<Option<WelcomeSocket>>,
}

impl LinkTransport for WelcomeTransport {
    fn connect<'call>(
        &'call self,
        token: &'call MachineTokenText,
    ) -> TransportFuture<'call, Result<Box<dyn LinkSocket>, LinkTransportError>> {
        assert_eq!(token, &self.expected_token);
        let socket = self.socket.lock().unwrap().take().expect("one connection");
        Box::pin(async move { Ok(Box::new(socket) as Box<dyn LinkSocket>) })
    }
}

struct WelcomeSocket {
    welcome: Option<String>,
    welcome_consumed: Option<oneshot::Sender<()>>,
}

impl LinkSocket for WelcomeSocket {
    fn send_text(&mut self, _: String) -> TransportFuture<'_, Result<(), LinkTransportError>> {
        Box::pin(async { Ok(()) })
    }

    fn next_inbound(&mut self) -> TransportFuture<'_, LinkInbound> {
        Box::pin(async {
            if let Some(welcome) = self.welcome.take() {
                return LinkInbound::Text(welcome);
            }
            // The driver polls this stream again after processing Welcome.
            if let Some(consumed) = self.welcome_consumed.take() {
                let _ = consumed.send(());
            }
            std::future::pending().await
        })
    }

    fn close(&mut self) -> TransportFuture<'_, ()> {
        Box::pin(async {})
    }
}

#[tokio::test]
async fn configured_host_connects_with_its_persisted_machine_id() {
    let root = tempfile::Builder::new()
        .prefix("host-link-")
        .tempdir_in("/tmp")
        .unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let machine_id = collaboration_runtime::load_or_create_machine_id(
        root.path(),
        runtime_primitives::SyncTime::from_unix_millis(0),
    )
    .await
    .unwrap();
    let token: MachineTokenText = "fixture.machine.token".to_owned().try_into().unwrap();
    let (consumed, welcomed) = oneshot::channel();
    let welcome = encode_text(
        &LinkFrame::Data(DataFrame::Welcome {
            machine_id: machine_id.clone(),
            central_time: "2030-01-01T00:00:00Z".parse().unwrap(),
        }),
        FrameDirection::ToRouter,
    )
    .unwrap();
    let runtime = CollaborationRuntime::start_for_host_with_router_proxy_endpoint(
        HostCollaborationInputs {
            enrollment_reset_directory: root.path().to_owned(),
            collaboration_runtime: runtime_inputs(root.path()),
            central_link: Some(ConfiguredCentralLink {
                tokens: Arc::new(FixtureTokens(token.clone())),
                transport: Arc::new(WelcomeTransport {
                    expected_token: token,
                    socket: Mutex::new(Some(WelcomeSocket {
                        welcome: Some(welcome),
                        welcome_consumed: Some(consumed),
                    })),
                }),
            }),
            router_proxy_endpoint: SocketAddr::from(([127, 0, 0, 1], 0)),
            remote_access: None,
        },
        Vec::new(),
        Arc::new(collaboration_mcp::FixedRouterExecutableRelation(
            RouterExecutableRelation::Match,
        )),
    )
    .await
    .unwrap();
    let link = runtime.central_link().expect("started Host link");
    let status = link.link_status().await;
    if status
        == Some(CentralLinkStatus::Refused {
            reason: CentralLinkRefusal::CredentialRefused,
        })
    {
        runtime.shutdown().await.unwrap();
        panic!("Host discarded the configured central link");
    }
    welcomed.await.expect("driver consumed the fixture Welcome");
    assert_eq!(
        link.link_status().await,
        Some(CentralLinkStatus::Connected { machine_id })
    );
    runtime.shutdown().await.unwrap();
    assert_eq!(link.link_status().await, None);
}
