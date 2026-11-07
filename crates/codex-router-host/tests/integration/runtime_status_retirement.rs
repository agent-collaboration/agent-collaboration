//! Pending operator exchanges survive cancellation during a real runtime replacement.
use codex_router_host::{
    AppServerLaunchPlan, ChildCommandSpec, ChildOutput, HostConfig, HostConfigInputs,
    HostCoordinationPaths, HostDeadlineInputs, HostDeadlines, HostRuntime, ManagedChildLaunchPlans,
    ManagedUpdateInputs, OperatorFrame, OperatorRequest, TerminalClassification,
};
use std::{os::unix::fs::PermissionsExt as _, path::Path, sync::Arc, time::Duration};
use tokio::{io::AsyncReadExt as _, io::AsyncWriteExt as _, sync::oneshot};

const BOUND: Duration = Duration::from_secs(20);
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[tokio::test]
async fn schema_replacement_answers_pending_status_and_await_host_start() -> TestResult {
    for request in [OperatorRequest::Status, OperatorRequest::AwaitHostStart] {
        assert_pending_request_is_answered(request).await?;
    }
    Ok(())
}

async fn assert_pending_request_is_answered(request: OperatorRequest) -> TestResult {
    // Arrange: the real Host, operator transport and collaboration runtime; only
    // the native executable and external router health endpoint are fixtures.
    let directory = tempfile::Builder::new()
        .prefix("host-status-retire-")
        .tempdir_in("/tmp")?;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    let router = GatedRouterProbe::start().await?;
    let paths = HostCoordinationPaths::for_router_root(directory.path());
    let socket = directory.path().join("native.sock");
    let executable = directory.path().join("codex-fixture");
    write_native_executable(&executable, 1)?;
    let identity = codex_native_integration::executable_identity(&executable).await?;
    let plan = AppServerLaunchPlan::new(
        ChildCommandSpec::new(executable.clone())
            .with_arguments([
                "--exact",
                &crate::child_test_selector(module_path!(), "native_child_entrypoint"),
                "--nocapture",
            ])
            .with_environment("CODEX_HOST_N1_NATIVE_SOCKET", &socket)
            .with_output(ChildOutput::Null),
        identity,
        "1.2.3".to_owned(),
    )
    .with_schema_directory(directory.path().join("schemas"));
    let collaboration = directory.path().join("collaboration");
    let config = HostConfig::new(HostConfigInputs {
        coordination_paths: paths.clone(),
        router_endpoint: router.address,
        mcp_bind: "127.0.0.1:0".parse()?,
        app_server_socket: socket,
        managed_executable: executable.clone(),
        deadlines: HostDeadlines::new(HostDeadlineInputs {
            router_start: Duration::from_secs(30),
            app_server_start: BOUND,
            remote_control: Duration::from_secs(2),
            endpoint_inspection: Duration::from_secs(1),
            operator_request: BOUND,
        })?,
    })
    .with_collaboration_directory(collaboration.clone(), collaboration.clone());
    let runtime = tokio::spawn(HostRuntime::run(
        config,
        ManagedChildLaunchPlans::new(None, plan),
        ManagedUpdateInputs::production(),
    ));
    let startup = crate::operator_client::send_operator_request(
        paths.operator_socket(),
        OperatorRequest::AwaitHostStart,
        BOUND,
    )
    .await?;
    assert_eq!(
        terminal(&startup)?.classification(),
        TerminalClassification::Ready
    );
    let before = manifest(&collaboration)?;
    assert!(
        before.native_schema_digest.is_some(),
        "fixture schema was not exported"
    );
    let (entered, release) = router.hold_next_probe()?;
    let pending = crate::operator_client::send_operator_request(
        paths.operator_socket(),
        request.clone(),
        BOUND,
    );
    tokio::pin!(pending);
    tokio::select! {
        result = &mut pending => panic!("status completed before the held probe: {result:?}"),
        result = tokio::time::timeout(BOUND, entered) => result??,
    }

    // Act: change the native schema, then use the real restart operation. Its
    // next lifecycle synchronization must retire and recreate collaboration.
    write_native_executable(&executable, 2)?;
    let (pending_result, restart_result) = tokio::join!(
        &mut pending,
        crate::operator_client::send_operator_request(
            paths.operator_socket(),
            OperatorRequest::RestartAppServer,
            BOUND,
        ),
    );
    let _released = release.send(());
    let after_status = crate::operator_client::send_operator_request(
        paths.operator_socket(),
        OperatorRequest::Status,
        BOUND,
    )
    .await;
    let after = manifest(&collaboration);

    // Always converge real children and storage before asserting the regression.
    // nextest isolates this test process; the Host's installed signal handler
    // owns the same orderly shutdown used by its StopHost operation.
    rustix::process::kill_process(rustix::process::getpid(), rustix::process::Signal::TERM)?;
    tokio::time::timeout(BOUND, runtime).await???;
    router.finish().await?;

    // Assert: runtime really changed, Host stayed usable, and the cancelled
    // request receives its own exactly-one terminal instead of EOF/MissingTerminal.
    assert_eq!(
        terminal(&restart_result?)?.classification(),
        TerminalClassification::Succeeded
    );
    let after = after?;
    assert_ne!(before.native_schema_digest, after.native_schema_digest);
    assert_ne!(before.service_epoch, after.service_epoch);
    assert_eq!(before.service_id, after.service_id);
    assert_eq!(
        terminal(&after_status?)?.classification(),
        TerminalClassification::Ready
    );
    let frames = pending_result.map_err(|error| {
        std::io::Error::other(format!(
            "runtime retirement omitted the operator terminal: {error}"
        ))
    })?;
    let answer = terminal(&frames)?;
    assert_eq!(answer.request(), &request);
    assert_eq!(answer.classification(), TerminalClassification::Unavailable);
    Ok(())
}

fn terminal(frames: &[OperatorFrame]) -> TestResult<&codex_router_host::HostTerminalResponse> {
    let mut answers = frames.iter().filter_map(|frame| match frame {
        OperatorFrame::Terminal(answer) => Some(answer),
        OperatorFrame::Progress(_) => None,
    });
    let answer = answers
        .next()
        .ok_or_else(|| std::io::Error::other(format!("operator terminal missing: {frames:?}")))?;
    if answers.next().is_some() {
        return Err(std::io::Error::other(format!("exactly one terminal: {frames:?}")).into());
    }
    Ok(answer)
}

fn manifest(directory: &Path) -> TestResult<collaboration_protocol::ServiceManifest> {
    Ok(serde_json::from_slice(&std::fs::read(
        directory.join("service.json"),
    )?)?)
}

fn write_native_executable(path: &Path, revision: u8) -> std::io::Result<()> {
    let mut definitions = serde_json::Map::new();
    for name in ["ThreadRead", "ThreadStart", "TurnStart"] {
        definitions.insert(
            format!("{name}Params"),
            serde_json::json!({
                "type":"object", "description":format!("fixture revision {revision}")
            }),
        );
        definitions.insert(
            format!("{name}Response"),
            serde_json::json!({"type":"object"}),
        );
    }
    let schema = serde_json::json!({"definitions":{"v2":definitions}});
    std::fs::write(
        path,
        format!(
            "#!/bin/sh\n# revision {revision}\nif [ \"$1\" = --version ]; then echo 'codex-cli 1.2.3';\nelif [ \"$2\" = generate-json-schema ]; then\ncat > \"$5/codex_app_server_protocol.schemas.json\" <<'SCHEMA'\n{schema}\nSCHEMA\nelse exec '{}' \"$@\"; fi\n",
            std::env::current_exe()?.display(),
        ),
    )?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

#[tokio::test]
async fn native_child_entrypoint() -> TestResult {
    let Some(socket) = std::env::var_os("CODEX_HOST_N1_NATIVE_SOCKET") else {
        return Ok(());
    };
    codex_router_test_support::native_app_server::run_native_app_server_fixture(
        Path::new(&socket),
        "1.2.3",
        None,
    )
    .await
}

struct HeldProbe {
    entered: oneshot::Sender<()>,
    release: oneshot::Receiver<()>,
}

struct GatedRouterProbe {
    address: std::net::SocketAddr,
    gate: Arc<std::sync::Mutex<Option<HeldProbe>>>,
    stop: oneshot::Sender<()>,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
}

impl GatedRouterProbe {
    async fn start() -> std::io::Result<Self> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let gate = Arc::new(std::sync::Mutex::new(None::<HeldProbe>));
        let server_gate = Arc::clone(&gate);
        let (stop, mut stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    _ = &mut stopped => break,
                    Some(completed) = connections.join_next(), if !connections.is_empty() => {
                        completed.map_err(std::io::Error::other)??;
                    },
                    accepted = listener.accept() => {
                        let (mut stream, _) = accepted?;
                        let held = server_gate.lock()
                            .map_err(|_| std::io::Error::other("router gate poisoned"))?.take();
                        connections.spawn(async move {
                            let mut request = [0_u8; 1024];
                            if stream.read(&mut request).await? == 0 {
                                return Ok::<_, std::io::Error>(());
                            }
                            if let Some(held) = held {
                                let _entered = held.entered.send(());
                                let _released = held.release.await;
                            }
                            let body = serde_json::to_vec(
                                &codex_router_core::router_compatibility::RouterCompatibility::current(false)
                            ).map_err(std::io::Error::other)?;
                            let header = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                            if stream.write_all(header.as_bytes()).await.is_ok() {
                                let _written = stream.write_all(&body).await;
                            }
                            Ok(())
                        });
                    }
                }
            }
            connections.shutdown().await;
            Ok(())
        });
        Ok(Self {
            address,
            gate,
            stop,
            task,
        })
    }

    fn hold_next_probe(&self) -> std::io::Result<(oneshot::Receiver<()>, oneshot::Sender<()>)> {
        let (entered, observed) = oneshot::channel();
        let (release, released) = oneshot::channel();
        *self
            .gate
            .lock()
            .map_err(|_| std::io::Error::other("router gate poisoned"))? = Some(HeldProbe {
            entered,
            release: released,
        });
        Ok((observed, release))
    }

    async fn finish(self) -> TestResult {
        self.stop
            .send(())
            .map_err(|()| std::io::Error::other("router fixture stopped early"))?;
        self.task.await??;
        Ok(())
    }
}
