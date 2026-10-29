use super::*;
use codex_router_host::{HostInstanceLock, InstanceAcquireError};

#[test]
fn stop_parses_default_and_explicit_router_roots() {
    let default = HostCommand::parse(vec![OsString::from("stop")]).expect("stop command");
    assert!(!default.runs_foreground());
    assert_eq!(default.router_root(), None);
    for arguments in [
        ["stop", "--router-root", "/tmp/stop-fixture"],
        ["--router-root", "/tmp/stop-fixture", "stop"],
    ] {
        let command =
            HostCommand::parse(arguments.map(OsString::from).to_vec()).expect("explicit stop root");
        assert!(!command.runs_foreground());
        assert_eq!(command.router_root(), Some(Path::new("/tmp/stop-fixture")));
    }
}

#[tokio::test]
async fn stop_absent_host_succeeds_without_creating_router_state() {
    let directory = tempfile::tempdir().expect("fixture");
    let root = directory.path().join("absent-router");
    let command = HostCommand::parse(vec![
        "stop".into(),
        "--router-root".into(),
        root.as_os_str().to_owned(),
    ])
    .expect("stop command");
    let mut output = Vec::new();
    run_host_command(&mut output, command, &CliContext::new(vec![]), None)
        .await
        .expect("idempotent stop");
    assert_eq!(String::from_utf8(output).unwrap(), "Host is not running.\n");
    assert!(!root.exists());
}

#[tokio::test]
async fn stop_does_not_report_absent_while_an_owner_holds_the_lock() {
    let directory = tempfile::tempdir().expect("fixture");
    let paths = HostCoordinationPaths::for_router_root(directory.path());
    let _owner = HostInstanceLock::acquire(&paths).expect("retained owner");
    let command = HostCommand::parse(vec![
        "stop".into(),
        "--router-root".into(),
        directory.path().as_os_str().to_owned(),
    ])
    .expect("stop command");
    let mut output = Vec::new();
    let error = run_host_command(&mut output, command, &CliContext::new(vec![]), None)
        .await
        .expect_err("unreachable owner must not be called absent");
    assert!(error.to_string().contains("lock"), "{error}");
    assert!(output.is_empty());
    assert!(matches!(
        HostInstanceLock::acquire(&paths),
        Err(InstanceAcquireError::AlreadyRunning)
    ));
}

#[tokio::test]
async fn stop_times_out_without_claiming_success_or_signalling_an_unresponsive_peer() {
    use tokio::io::AsyncReadExt;

    let directory = tempfile::tempdir().expect("fixture");
    let paths = HostCoordinationPaths::for_router_root(directory.path());
    let _owner = HostInstanceLock::acquire(&paths).expect("retained owner");
    let listener = tokio::net::UnixListener::bind(directory.path().join("host.sock"))
        .expect("operator fixture");
    let command = HostCommand::parse(vec![
        "stop".into(),
        "--router-root".into(),
        directory.path().as_os_str().to_owned(),
    ])
    .expect("stop command");
    let operation = tokio::spawn(async move {
        let mut output = Vec::new();
        let result = run_host_command(&mut output, command, &CliContext::new(vec![]), None).await;
        (result, output)
    });
    let (mut connection, _) = listener.accept().await.expect("stop connection");
    let mut request = Vec::new();
    connection
        .read_to_end(&mut request)
        .await
        .expect("complete stop request");
    assert!(!request.is_empty());
    // The fixture has received the request but deliberately keeps its process alive.
    let (result, output) = operation.await.expect("command task");
    let error = result.expect_err("peer did not exit");
    assert!(error.to_string().contains("did not exit"), "{error}");
    assert!(output.is_empty());
}
