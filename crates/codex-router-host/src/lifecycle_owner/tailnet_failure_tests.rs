use super::*;
use tokio::io::AsyncReadExt as _;

#[tokio::test]
async fn tailscale_failure_settles_actual_owned_children_before_returning_host_error() {
    let directory = tempfile::tempdir_in("/tmp").expect("short control socket");
    let socket_path = directory.path().join("child.sock");
    let listener = tokio::net::UnixListener::bind(&socket_path).expect("readiness listener");
    let child_script = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../verification/fixtures/held-socket-child.ts"
    );
    let node_executable = node_test_fixture::node_executable().expect("fixture Node executable");
    let make_command = || {
        let mut command = tokio::process::Command::new(&node_executable);
        command
            .args([child_script])
            .arg(&socket_path)
            .kill_on_drop(true);
        command
    };
    let identity = codex_native_integration::executable_identity(&node_executable)
        .await
        .expect("fixture executable");
    let app_process =
        ProcessGroupChild::spawn(&mut make_command()).expect("owned app-server stand-in");
    let app_pid = app_process.process_id();
    let mut app_server = Some(AppServerChild::new(app_process, identity));
    let router_child = RouterChild::spawn(&mut make_command()).expect("owned router stand-in");
    let router_pid = router_child.process_id();
    let mut router = Some(router_child);
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().await.expect("fixture ready connection");
            assert_eq!(stream.read_u8().await.expect("fixture ready event"), b'R');
        }
    })
    .await
    .expect("both actual child processes reach the held step");

    let result = settle_failed_tailnet_exposure(
        std::io::Error::other("owned Serve child exited after ready"),
        ShutdownContext {
            activation: &mut None,
            active_update: &mut None,
            pending_identity: &mut None,
            retained_updater: &mut None,
            active_app_server_restart: &mut None,
            active_router_restart: &mut None,
            recovery: &mut None,
            app_server: &mut app_server,
            router: &mut router,
        },
    )
    .await;
    assert!(
        matches!(result, Err(HostError::Collaboration(error)) if error.to_string() == "owned Serve child exited after ready")
    );
    for process_id in [app_pid, router_pid] {
        let pid = rustix::process::Pid::from_raw(i32::try_from(process_id).expect("pid fits"))
            .expect("pid");
        assert!(
            rustix::process::test_kill_process(pid).is_err(),
            "owned process {process_id} survived fatal exposure settlement"
        );
    }
}
