//! Real private frontend and child supervision with a native-wire fixture process.
//! This does not stand in for the final real Codex/two-machine action proof.
use super::*;
use collaboration_client::protocol::EndpointRef;
use serde_json::json;
use std::path::PathBuf;
use tokio::io::AsyncReadExt;

const CLIENT_PROBE: &str = "sessions::source_native_carrier::tests::source_carrier_client_probe";
const SUPERVISOR_PROBE: &str =
    "sessions::source_native_carrier::tests::source_carrier_supervisor_probe";
const INITIALIZE: &str = "{\"id\":\"carrier-proof\",\"method\":\"initialize\"}";
const REPLY: &str = "{\"id\":\"carrier-proof\",\"result\":{}}";
const DRAIN_PAYLOAD_BYTE: u8 = 0x5a;

fn prepared_launch(directory: &Path, action: &str) -> PreparedSourceLaunch {
    let directory = std::fs::canonicalize(directory).unwrap_or_else(|_| directory.to_owned());
    let action = match action {
        "new" => json!({"kind":"new","directory":directory}),
        kind => json!({"kind":kind,"target":{
            "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000021","endpointId":"codex-local"},
            "sessionId":"source-session-id"},"storedModel":{"model":"stored-model","reasoningEffort":"high"}}),
    };
    serde_json::from_value(json!({
        "action":action,
        "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000021","endpointId":"codex-local"},
        "generation":{"serviceEpoch":"00000000-0000-4000-8000-000000000022","generation":1},
        "schemaDigest":format!("sha256:{}", "b".repeat(64)),
        "workingDirectory":directory,"clientProfile":"routerDebug"
    }))
    .unwrap()
}

fn client_probe_command(
    arguments: &[OsString],
    mode: &str,
    record: &Path,
) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    let arguments: Vec<_> = arguments
        .iter()
        .map(|argument| argument.to_str().unwrap())
        .collect();
    command
        .args(["--exact", CLIENT_PROBE, "--ignored", "--nocapture"])
        .env(
            "SOURCE_CARRIER_FIXTURE_ARGUMENTS",
            serde_json::to_string(&arguments).unwrap(),
        )
        .env("SOURCE_CARRIER_FIXTURE_MODE", mode)
        .env("SOURCE_CARRIER_FIXTURE_RECORD", record);
    command
}

fn assert_private_frontend_removed(record: &Path) {
    let observation: serde_json::Value =
        serde_json::from_slice(&std::fs::read(record).unwrap()).unwrap();
    let frontend = Path::new(observation["frontend"].as_str().unwrap());
    assert!(!frontend.exists(), "owned frontend socket remains");
    assert!(
        !frontend.parent().unwrap().exists(),
        "owned private directory remains"
    );
    let pid = observation["pid"].as_u64().unwrap();
    let exists = std::process::Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(
        !exists.success(),
        "owned native-wire child remains after supervisor returned"
    );
}

async fn native_connection_pair(
    endpoint: EndpointRef,
) -> (NativeTransportConnection, WebSocketStream<UnixStream>) {
    let (caller, backend) = UnixStream::pair().unwrap();
    let (caller, backend) = tokio::join!(
        tokio_tungstenite::client_async("ws://localhost/", caller),
        tokio_tungstenite::accept_async(backend),
    );
    (
        NativeTransportConnection {
            endpoint,
            stream: caller.unwrap().0,
        },
        backend.unwrap(),
    )
}

#[test]
fn source_carrier_arguments_keep_source_target_profile_cwd_and_explicit_model_precedence() {
    let frontend = Path::new("/source/private/native.sock");
    let directory = Path::new("/source/project");
    let defaults = SourceNativeOptions::default();
    let expected_prefix = [
        "--profile",
        "codex-router-debug",
        "--remote",
        "unix:///source/private/native.sock",
        "--cd",
        "/source/project",
    ];
    for action in ["new", "resume", "fork"] {
        let prepared = prepared_launch(directory, action);
        let actual = native_arguments(&prepared, &defaults, frontend);
        let actual: Vec<_> = actual.iter().map(|value| value.to_str().unwrap()).collect();
        let mut expected = expected_prefix.to_vec();
        if action != "new" {
            expected.extend([
                "-c",
                "model=\"stored-model\"",
                "-c",
                "model_reasoning_effort=\"high\"",
                action,
                "--",
                "source-session-id",
            ]);
        }
        assert_eq!(actual, expected);
    }
    let prepared = prepared_launch(directory, "fork");
    let options = SourceNativeOptions {
        model: Some("explicit-model".to_owned()),
        reasoning_effort: Some("low".to_owned()),
        no_alt_screen: true,
    };
    let actual = native_arguments(&prepared, &options, frontend);
    let actual: Vec<_> = actual.iter().map(|value| value.to_str().unwrap()).collect();
    let mut expected = expected_prefix.to_vec();
    expected.extend([
        "--model=explicit-model",
        "-c",
        "model_reasoning_effort=\"low\"",
        "--no-alt-screen",
        "fork",
        "--",
        "source-session-id",
    ]);
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn source_carrier_child_exit_before_connect_reaps_process_and_removes_private_socket() {
    let root = tempfile::tempdir().unwrap();
    let prepared = prepared_launch(root.path(), "new");
    let record = root.path().join("child.json");
    let (connection, mut backend) = native_connection_pair(prepared.endpoint.clone()).await;
    let result = tokio::time::timeout(
        Duration::from_secs(3),
        run_source_client_with_command(
            &prepared,
            &SourceNativeOptions::default(),
            connection,
            |arguments| client_probe_command(arguments, "exit-before-connect", &record),
            source_settlement_limit,
        ),
    )
    .await
    .unwrap();
    assert_eq!(result, Ok(0));
    let closed = tokio::time::timeout(Duration::from_secs(2), backend.next())
        .await
        .unwrap();
    assert!(matches!(
        closed,
        None | Some(Err(_)) | Some(Ok(Message::Close(_)))
    ));
    assert_private_frontend_removed(&record);
}

#[tokio::test]
async fn source_carrier_forwards_native_child_messages_once_and_cleans_up_after_child_exit() {
    let root = tempfile::tempdir().unwrap();
    let prepared = prepared_launch(root.path(), "resume");
    let record = root.path().join("child.json");
    let (connection, mut backend) = native_connection_pair(prepared.endpoint.clone()).await;
    let options = SourceNativeOptions::default();
    let (result, frames) = tokio::time::timeout(Duration::from_secs(3), async {
        tokio::join!(
            run_source_client_with_command(
                &prepared,
                &options,
                connection,
                |arguments| { client_probe_command(arguments, "exchange", &record) },
                source_settlement_limit
            ),
            async {
                let initialize = backend.next().await.unwrap().unwrap();
                assert_eq!(initialize, Message::Text(INITIALIZE.into()));
                backend.send(Message::Text(REPLY.into())).await.unwrap();
                let binary = backend.next().await.unwrap().unwrap();
                assert_eq!(binary, Message::Binary(vec![0, 255, 1, 17].into()));
                let closed = backend.next().await;
                assert!(matches!(
                    closed,
                    None | Some(Err(_)) | Some(Ok(Message::Close(_)))
                ));
                2
            },
        )
    })
    .await
    .unwrap();
    assert_eq!(result, Ok(0));
    assert_eq!(frames, 2);
    assert_private_frontend_removed(&record);
}

#[tokio::test]
async fn source_carrier_drains_buffered_frames_after_actual_child_exit() {
    let root = tempfile::Builder::new()
        .prefix("carrier-drain-")
        .tempdir_in("/tmp")
        .unwrap();
    let prepared = prepared_launch(root.path(), "resume");
    let record = root.path().join("child.json");
    let exit_socket = root.path().join("exit.sock");
    let exit_listener = UnixListener::bind(&exit_socket).unwrap();
    let (connection, mut backend) = native_connection_pair(prepared.endpoint.clone()).await;
    let payload_bytes = {
        let socket = socket2::SockRef::from(connection.stream.get_ref());
        socket.set_send_buffer_size(4096).unwrap();
        socket
            .send_buffer_size()
            .unwrap()
            .saturating_mul(16)
            .max(65_536)
    };
    assert!(payload_bytes < NATIVE_WEBSOCKET_FRAME_LIMIT);
    let options = SourceNativeOptions::default();
    let (result, frames) = tokio::time::timeout(Duration::from_secs(3), async {
        tokio::join!(
            run_source_client_with_command(&prepared, &options, connection, |arguments| {
                let mut command = client_probe_command(arguments, "buffered-exit", &record);
                command.env("SOURCE_CARRIER_EXIT_SOCKET", &exit_socket)
                    .env("SOURCE_CARRIER_PAYLOAD_BYTES", payload_bytes.to_string());
                command
            }, source_settlement_limit),
            async {
                assert_eq!(backend.next().await.unwrap().unwrap(), Message::Text(INITIALIZE.into()));
                backend.send(Message::Text(REPLY.into())).await.unwrap();
                let (mut exit_control, _) = exit_listener.accept().await.unwrap();
                let mut control_bytes = Vec::new();
                // The child never closes this descriptor in Rust. Its EOF is
                // the kernel's process-exit event, before the backend drains.
                exit_control.read_to_end(&mut control_bytes).await.unwrap();
                assert!(control_bytes.is_empty());
                let payload = backend.next().await.expect("buffered frame after child exit")
                    .expect("child exit must not cancel an in-flight backend write");
                assert!(matches!(payload, Message::Binary(ref bytes)
                    if bytes.len() == payload_bytes && bytes.iter().all(|byte| *byte == DRAIN_PAYLOAD_BYTE)),
                    "buffered payload must preserve every byte");
                assert_eq!(backend.next().await.unwrap().unwrap(), Message::Binary(vec![0, 255, 1, 17].into()));
                assert!(matches!(backend.next().await, None | Some(Err(_)) | Some(Ok(Message::Close(_)))));
                3
            },
        )
    }).await.expect("real child exit and forwarding must settle");
    assert_eq!(result, Ok(0));
    assert_eq!(frames, 3);
    assert_private_frontend_removed(&record);
}

#[tokio::test]
async fn source_carrier_backend_loss_settles_its_child_and_never_reconnects() {
    let root = tempfile::tempdir().unwrap();
    let prepared = prepared_launch(root.path(), "new");
    let record = root.path().join("child.json");
    let (connection, mut backend) = native_connection_pair(prepared.endpoint.clone()).await;
    let options = SourceNativeOptions::default();
    let (limit_started, limit_observed) = tokio::sync::oneshot::channel();
    let (expire_limit, limit_expired) = tokio::sync::oneshot::channel();
    let (result, ()) = tokio::time::timeout(Duration::from_secs(4), async {
        tokio::join!(
            run_source_client_with_command(
                &prepared,
                &options,
                connection,
                |arguments| { client_probe_command(arguments, "hold-until-loss", &record) },
                || async {
                    limit_started.send(()).unwrap();
                    limit_expired.await.unwrap();
                }
            ),
            async {
                assert_eq!(
                    backend.next().await.unwrap().unwrap(),
                    Message::Text(INITIALIZE.into())
                );
                backend.send(Message::Text(REPLY.into())).await.unwrap();
                assert_eq!(
                    backend.next().await.unwrap().unwrap(),
                    Message::Binary(vec![0, 255, 1, 17].into())
                );
                drop(backend);
                limit_observed
                    .await
                    .expect("backend loss must arm child settlement");
                let observation: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(&record).unwrap()).unwrap();
                let pid = observation["pid"].as_u64().unwrap();
                assert!(
                    std::process::Command::new("/bin/kill")
                        .args(["-0", &pid.to_string()])
                        .status()
                        .unwrap()
                        .success(),
                    "stalled child must remain owned before the settlement limit"
                );
                expire_limit.send(()).unwrap();
            },
        )
    })
    .await
    .unwrap();
    assert_eq!(result, Err(SourceRejection::SourceUnavailable));
    assert_private_frontend_removed(&record);
}

#[tokio::test]
async fn source_carrier_signals_reap_owned_child_and_private_socket() {
    for (signal_argument, expected_exit) in [("-TERM", 143), ("-INT", 130), ("-HUP", 129)] {
        let root = tempfile::Builder::new()
            .prefix("source-signal-")
            .tempdir_in("/tmp")
            .unwrap();
        let backend_path = root.path().join("backend.sock");
        let listener = UnixListener::bind(&backend_path).unwrap();
        let record = root.path().join("child.json");
        let prepared = prepared_launch(root.path(), "new");
        let mut supervisor = tokio::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", SUPERVISOR_PROBE, "--ignored", "--nocapture"])
            .env(
                "SOURCE_CARRIER_FIXTURE_PREPARED",
                serde_json::to_string(&prepared).unwrap(),
            )
            .env("SOURCE_CARRIER_FIXTURE_BACKEND", &backend_path)
            .env("SOURCE_CARRIER_FIXTURE_RECORD", &record)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let supervisor_pid = supervisor.id().unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            let (socket, _) = listener.accept().await.unwrap();
            let mut backend = tokio_tungstenite::accept_async(socket).await.unwrap();
            assert_eq!(
                backend.next().await.unwrap().unwrap(),
                Message::Text(INITIALIZE.into())
            );
            backend.send(Message::Text(REPLY.into())).await.unwrap();
            assert_eq!(
                backend.next().await.unwrap().unwrap(),
                Message::Binary(vec![0, 255, 1, 17].into())
            );
            // Only this freshly spawned handle is signalled, never a discovered PID.
            let status = tokio::process::Command::new("/bin/kill")
                .args([signal_argument, &supervisor_pid.to_string()])
                .status()
                .await
                .unwrap();
            assert!(status.success());
            let closed = backend.next().await;
            assert!(matches!(
                closed,
                None | Some(Err(_)) | Some(Ok(Message::Close(_)))
            ));
        })
        .await
        .unwrap();
        let status = tokio::time::timeout(Duration::from_secs(3), supervisor.wait())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(status.code(), Some(expected_exit));
        assert_private_frontend_removed(&record);
    }
}

#[test]
#[ignore = "native-wire fixture child, invoked only by the parent carrier tests"]
fn source_carrier_client_probe() {
    let arguments: Vec<String> =
        serde_json::from_str(&std::env::var("SOURCE_CARRIER_FIXTURE_ARGUMENTS").unwrap()).unwrap();
    let remote_index = arguments
        .iter()
        .position(|value| value == "--remote")
        .unwrap();
    let frontend = PathBuf::from(arguments[remote_index + 1].strip_prefix("unix://").unwrap());
    let cwd_index = arguments.iter().position(|value| value == "--cd").unwrap();
    assert_eq!(
        std::env::current_dir().unwrap(),
        Path::new(&arguments[cwd_index + 1])
    );
    assert_eq!(
        frontend.metadata().unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        frontend
            .parent()
            .unwrap()
            .metadata()
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    let record = std::env::var("SOURCE_CARRIER_FIXTURE_RECORD").unwrap();
    std::fs::write(
        record,
        serde_json::to_vec(&json!({"pid":std::process::id(),"frontend":frontend})).unwrap(),
    )
    .unwrap();
    let mode = std::env::var("SOURCE_CARRIER_FIXTURE_MODE").unwrap();
    if mode == "exit-before-connect" {
        return;
    }
    let exit_control = if mode == "buffered-exit" {
        Some(
            std::os::unix::net::UnixStream::connect(
                std::env::var_os("SOURCE_CARRIER_EXIT_SOCKET").expect("owned exit socket"),
            )
            .unwrap(),
        )
    } else {
        None
    };
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let socket = UnixStream::connect(&frontend).await.unwrap();
            let (mut stream, _) = tokio_tungstenite::client_async("ws://localhost/", socket)
                .await
                .unwrap();
            stream.send(Message::Text(INITIALIZE.into())).await.unwrap();
            assert_eq!(
                stream.next().await.unwrap().unwrap(),
                Message::Text(REPLY.into())
            );
            if mode == "buffered-exit" {
                let payload_bytes: usize = std::env::var("SOURCE_CARRIER_PAYLOAD_BYTES")
                    .unwrap()
                    .parse()
                    .unwrap();
                assert!(payload_bytes > 0 && payload_bytes < NATIVE_WEBSOCKET_FRAME_LIMIT);
                stream
                    .send(Message::Binary(
                        vec![DRAIN_PAYLOAD_BYTE; payload_bytes].into(),
                    ))
                    .await
                    .unwrap();
            }
            stream
                .send(Message::Binary(vec![0, 255, 1, 17].into()))
                .await
                .unwrap();
            if mode == "hold-until-loss" {
                let _closed = stream.next().await;
                // A stalled external client must be killed and reaped by its owner.
                std::future::pending::<()>().await;
            } else {
                stream.close(None).await.unwrap();
            }
        });
    if mode == "buffered-exit" {
        std::hint::black_box(&exit_control);
        std::process::exit(0);
    }
}

#[test]
#[ignore = "isolated signal fixture supervisor, invoked only by its parent test"]
fn source_carrier_supervisor_probe() {
    let prepared: PreparedSourceLaunch =
        serde_json::from_str(&std::env::var("SOURCE_CARRIER_FIXTURE_PREPARED").unwrap()).unwrap();
    let backend = std::env::var("SOURCE_CARRIER_FIXTURE_BACKEND").unwrap();
    let record = PathBuf::from(std::env::var("SOURCE_CARRIER_FIXTURE_RECORD").unwrap());
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let socket = UnixStream::connect(backend).await.unwrap();
            let (stream, _) = tokio_tungstenite::client_async("ws://localhost/", socket)
                .await
                .unwrap();
            run_source_client_with_command(
                &prepared,
                &SourceNativeOptions::default(),
                NativeTransportConnection {
                    endpoint: prepared.endpoint.clone(),
                    stream,
                },
                |arguments| client_probe_command(arguments, "hold-until-loss", &record),
                source_settlement_limit,
            )
            .await
        });
    std::process::exit(result.expect("signal must settle successfully"));
}
