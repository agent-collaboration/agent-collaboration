//! Real child-startup failures must remain diagnosable and joined on cleanup.
use super::*;
use std::{
    io::Write,
    sync::{Arc, Mutex},
    time::Duration,
};
use tracing::instrument::WithSubscriber;

#[derive(Clone, Default)]
struct StartupTrace(Arc<Mutex<Vec<u8>>>);

impl Write for StartupTrace {
    #[expect(
        clippy::unwrap_in_result,
        reason = "A poisoned test capture must fail the test instead of silently losing evidence."
    )]
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("trace lock").extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'writer> tracing_subscriber::fmt::MakeWriter<'writer> for StartupTrace {
    type Writer = Self;

    fn make_writer(&'writer self) -> Self::Writer {
        self.clone()
    }
}

async fn failed_startup_trace(script: &str, virtual_readiness_deadline: bool) -> String {
    let root = tempfile::tempdir().expect("owned fixture root");
    let endpoint = std::net::TcpListener::bind("127.0.0.1:0")
        .expect("unused endpoint")
        .local_addr()
        .expect("fixture address");
    let pid_path = root.path().join("child-pid");
    let config = HostConfig::new(crate::HostConfigInputs {
        coordination_paths: crate::HostCoordinationPaths::new(
            root.path().join("operator.sock"),
            root.path().join("host.lock"),
        ),
        router_endpoint: endpoint,
        mcp_bind: "127.0.0.1:0".parse().expect("MCP endpoint"),
        app_server_socket: root.path().join("native.sock"),
        managed_executable: "/unused/codex".into(),
        deadlines: crate::HostDeadlines::new(crate::HostDeadlineInputs {
            router_start: Duration::from_millis(250),
            app_server_start: Duration::from_secs(1),
            remote_control: Duration::from_secs(1),
            endpoint_inspection: Duration::from_secs(1),
            operator_request: Duration::from_secs(1),
        })
        .expect("bounded fixture deadlines"),
    });
    let command = ChildCommandSpec::new("/bin/sh".into())
        .with_arguments([
            "-c".into(),
            script.into(),
            "fixture".into(),
            pid_path.into_os_string(),
        ])
        .with_output(crate::ChildOutput::Null);
    let trace = StartupTrace::default();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_max_level(tracing::Level::DEBUG)
        .with_writer(trace.clone())
        .finish();
    let child_recording = if virtual_readiness_deadline {
        let mut events =
            crate::native_file_events::NativeFileEvents::open(&root.path().join("child-pid"))
                .expect("register PID observation before launch");
        let (recorded, observed) = std::sync::mpsc::channel();
        let watcher = tokio::spawn(async move {
            let recorded_pid = loop {
                if std::fs::read_to_string(events.path())
                    .is_ok_and(|text| text.trim().parse::<u32>().is_ok())
                {
                    break true;
                }
                if events.wait_for_change().await.is_err() {
                    break false;
                }
            };
            let _recorded = recorded.send(recorded_pid);
        });
        let recording = tokio::task::spawn_blocking(move || {
            observed
                .recv_timeout(Duration::from_secs(1))
                .unwrap_or(false)
        });
        tokio::time::pause();
        Some((watcher, recording))
    } else {
        None
    };
    let startup = super::start_router(&config, Some(&command)).with_subscriber(subscriber);
    tokio::pin!(startup);
    let outcome = if let Some((watcher, mut recording)) = child_recording {
        let (outcome, recorded) = tokio::select! {
            biased;
            outcome = &mut startup => {
                tokio::time::resume();
                (outcome, recording.await.expect("PID observation guard joins"))
            }
            recorded = &mut recording => {
                let recorded = recorded.expect("PID observation guard joins");
                // Hold startup across readiness expiry; native reaping needs running time.
                tokio::time::advance(config.deadlines().router_start()).await;
                tokio::time::resume();
                (startup.await, recorded)
            }
        };
        watcher.abort();
        let _joined = watcher.await;
        assert!(
            recorded,
            "real child recorded its PID before virtual readiness expiry"
        );
        outcome
    } else {
        startup.await
    };
    assert!(matches!(outcome, Err(HostError::RouterUnavailable)));
    let child_pid = std::fs::read_to_string(root.path().join("child-pid"))
        .expect("fixture child ran")
        .trim()
        .to_owned();
    let remaining = std::process::Command::new("/bin/kill")
        .args(["-0", &child_pid])
        .output()
        .expect("read-only child liveness check");
    assert!(
        !remaining.status.success(),
        "startup failure must reap its child"
    );
    let bytes = trace.0.lock().expect("trace lock").clone();
    String::from_utf8(bytes).expect("UTF-8 diagnostic")
}

#[tokio::test]
async fn router_startup_reports_early_exit_and_reaps_owned_child() {
    let trace = failed_startup_trace("echo $$ > \"$1\"; exit 23", false).await;
    assert!(trace.contains("child_state=\"exited\""), "{trace}");
    assert!(trace.contains("exit_code=Some(23)"), "{trace}");
    assert!(trace.contains("elapsed_ms="), "{trace}");
    assert!(trace.contains("connect_error=ConnectionRefused"), "{trace}");
}

#[tokio::test]
async fn router_startup_reports_exit_signal_and_reaps_owned_child() {
    let trace = failed_startup_trace("echo $$ > \"$1\"; kill -TERM $$", false).await;
    assert!(trace.contains("child_state=\"exited\""), "{trace}");
    assert!(trace.contains("exit_code=None"), "{trace}");
    assert!(trace.contains("exit_signal=Some(15)"), "{trace}");
}

#[tokio::test]
async fn router_startup_reports_live_nonlistener_and_reaps_owned_child() {
    let trace = failed_startup_trace("echo $$ > \"$1\"; exec /bin/sleep 30", true).await;
    assert!(trace.contains("child_state=\"running\""), "{trace}");
    assert!(trace.contains("elapsed_ms="), "{trace}");
    assert!(trace.contains("connect_error=ConnectionRefused"), "{trace}");
}
