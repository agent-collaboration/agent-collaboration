//! Real operator sockets check capability negotiation, immediate display and terminal meanings.
use std::io::{self, Write};

use codex_router_host::{
    AppServerCondition, ExecutableRelation, HostPhase, HostSnapshot, HostSnapshotDimensions,
    HostTerminalResponse, RecoveryBudget, RemoteControlCondition, RouterCondition,
    TerminalClassification, decode_operator_request, encode_operator_frame,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    sync::oneshot,
};

use super::*;

fn snapshot() -> HostSnapshot {
    HostSnapshot::new(HostSnapshotDimensions {
        phase: HostPhase::Steady,
        router: RouterCondition::OwnedReachable,
        app_server: AppServerCondition::NativeReady {
            running_version: "fixture".to_owned(),
        },
        remote_control: RemoteControlCondition::Connected,
        remote_control_identity: None,
        executable_relation: ExecutableRelation::Match,
        router_executable_relation: codex_router_host::RouterExecutableRelation::Match,
        recovery_budget: RecoveryBudget::Available,
        last_lifecycle_outcome: None,
    })
}
fn status_frame() -> OperatorFrame {
    OperatorFrame::Terminal(HostTerminalResponse::new(
        OperatorRequest::Status,
        TerminalClassification::Ready,
        snapshot(),
        "Ready".to_owned(),
    ))
}
fn login_request() -> OperatorRequest {
    OperatorRequest::FleetLogin {
        organization_id: "019f0000-0000-7000-8000-000000000001"
            .to_owned()
            .try_into()
            .unwrap(),
        label: "Fixture machine".to_owned().try_into().unwrap(),
    }
}
fn code() -> FleetLoginEvent {
    FleetLoginEvent::Code {
        user_code: "ABCD-EFGH".to_owned().try_into().unwrap(),
        verification_uri: "https://sign-in.example.invalid/activate"
            .to_owned()
            .try_into()
            .unwrap(),
        keeping: RefreshCredentialKeeping::MemoryOnly,
    }
}
async fn read_request(stream: &mut UnixStream) -> OperatorRequest {
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).await.unwrap();
    decode_operator_request(&bytes).unwrap()
}
async fn answer_status(listener: &UnixListener, frame: OperatorFrame) {
    let (mut stream, _) = listener.accept().await.unwrap();
    assert_eq!(read_request(&mut stream).await, OperatorRequest::Status);
    stream
        .write_all(&encode_operator_frame(&frame).unwrap())
        .await
        .unwrap();
}

#[tokio::test]
async fn fleet_machine_login_cli_old_host_is_typed_unsupported_before_sending_login() {
    let root = tempfile::tempdir().unwrap();
    let socket = root.path().join("operator.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let mut wire = serde_json::to_value(status_frame()).unwrap();
    wire["payload"]["snapshot"]
        .as_object_mut()
        .unwrap()
        .remove("operator_capabilities");
    let old_frame: OperatorFrame = serde_json::from_value(wire).unwrap();
    let server = tokio::spawn(async move {
        answer_status(&listener, old_frame).await;
    });

    let result = exchange(&socket, login_request(), &mut Vec::new()).await;

    assert!(matches!(
        result,
        Err(FleetLoginCommandError::Refused(
            FleetLoginRefusal::HostUnsupported {}
        ))
    ));
    assert_eq!(
        unsupported().to_string(),
        "this host doesn't support fleet login; upgrade codex-router"
    );
    server.await.unwrap();
}

struct ObservedOutput {
    bytes: Vec<u8>,
    displayed: Option<oneshot::Sender<()>>,
}
impl Write for ObservedOutput {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        if String::from_utf8_lossy(&self.bytes).contains("ABCD-EFGH")
            && let Some(displayed) = self.displayed.take()
        {
            let _sent = displayed.send(());
        }
        Ok(())
    }
}

#[tokio::test]
async fn fleet_machine_login_cli_flushes_code_before_terminal_on_the_real_socket() {
    let root = tempfile::tempdir().unwrap();
    let socket = root.path().join("operator.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let (displayed, observed) = oneshot::channel();
    let server = tokio::spawn(async move {
        answer_status(&listener, status_frame()).await;
        let (mut stream, _) = listener.accept().await.unwrap();
        assert_eq!(read_request(&mut stream).await, login_request());
        stream
            .write_all(&encode_operator_frame(&OperatorFrame::FleetLogin(code())).unwrap())
            .await
            .unwrap();
        observed.await.unwrap();
        let terminal = FleetLoginEvent::LoggedIn {
            organization_id: "019f0000-0000-7000-8000-000000000001"
                .to_owned()
                .try_into()
                .unwrap(),
            enrollment_id: "019f0000-0000-7000-8000-000000000010"
                .to_owned()
                .try_into()
                .unwrap(),
            keeping: RefreshCredentialKeeping::MemoryOnly,
        };
        stream
            .write_all(&encode_operator_frame(&OperatorFrame::FleetLogin(terminal)).unwrap())
            .await
            .unwrap();
    });
    let mut output = ObservedOutput {
        bytes: Vec::new(),
        displayed: Some(displayed),
    };

    exchange(&socket, login_request(), &mut output)
        .await
        .unwrap();

    let text = String::from_utf8(output.bytes).unwrap();
    assert!(text.contains("Open https://sign-in.example.invalid/activate\nEnter code: ABCD-EFGH"));
    assert!(text.contains("approve this machine again after a restart"));
    assert!(text.contains("Logged in to organization"));
    assert!(!text.contains("device_code"));
    assert!(!text.contains("refresh_token"));
    server.await.unwrap();
}

#[tokio::test]
async fn fleet_machine_login_cli_distinguishes_unknown_ending_from_an_interrupted_host_stream() {
    for terminal in [
        None,
        Some(FleetLoginEvent::OutcomeUnknown {
            context: FleetLoginUnknown::DeviceRedemption {
                organization_id: "019f0000-0000-7000-8000-000000000001"
                    .to_owned()
                    .try_into()
                    .unwrap(),
                installation: "019f0000-0000-7000-8000-000000000002"
                    .to_owned()
                    .try_into()
                    .unwrap(),
            },
        }),
    ] {
        let expect_unknown = terminal.is_some();
        let root = tempfile::tempdir().unwrap();
        let socket = root.path().join("operator.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = tokio::spawn(async move {
            answer_status(&listener, status_frame()).await;
            let (mut stream, _) = listener.accept().await.unwrap();
            assert_eq!(read_request(&mut stream).await, login_request());
            stream
                .write_all(&encode_operator_frame(&OperatorFrame::FleetLogin(code())).unwrap())
                .await
                .unwrap();
            if let Some(terminal) = terminal {
                stream
                    .write_all(
                        &encode_operator_frame(&OperatorFrame::FleetLogin(terminal)).unwrap(),
                    )
                    .await
                    .unwrap();
            }
        });

        let result = exchange(&socket, login_request(), &mut Vec::new()).await;

        if expect_unknown {
            assert!(matches!(
                result,
                Err(FleetLoginCommandError::OutcomeUnknown(_))
            ));
            let error = result.unwrap_err().to_string();
            assert!(error.contains("outcome unknown"));
            assert!(error.contains("run codex-router fleet login again"));
            assert!(!error.contains("previous login"));
        } else {
            assert!(matches!(result, Err(FleetLoginCommandError::Interrupted)));
            assert_eq!(
                result.unwrap_err().to_string(),
                "login interrupted, run it again"
            );
        }
        server.await.unwrap();
    }
}
