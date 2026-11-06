use agent_session_inventory::test_support::fake_api_support::{FakeCollaborationApi, FakeReply};
use serde_json::{Value, json};

const SERVICE: &str = "019f0000-0000-7000-8000-000000000001";
const REQUEST: &str = "019f0000-0000-7000-8000-000000000002";
const TARGET: &str = "019f0000-0000-7000-8000-000000000003";

#[tokio::test]
async fn remote_create_cli_preserves_every_typed_admission_and_first_input() {
    let mut replies = vec![
        json!({"kind":"accepted","requestId":REQUEST}),
        json!({"kind":"replayed","requestId":REQUEST,"status":{"kind":"admitted"}}),
    ];
    replies.extend(
        [
            "senderNotEnrolled",
            "targetNotEnrolled",
            "targetOffline",
            "conflict",
            "tooOld",
        ]
        .map(|kind| json!({"kind":"refused","reason":{"kind":kind}})),
    );
    for expected in replies {
        let mut api = FakeCollaborationApi::new(SERVICE, REQUEST).unwrap();
        let served = api.serve(vec![FakeReply::Result(expected.clone())]);
        let creator = json!({"endpoint":{"serviceId":SERVICE,"endpointId":"claude-local"},"sessionId":"requester"}).to_string();
        let output = tokio::process::Command::new(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../scripts/cargo_debug_signing_runner.sh"),
        )
        .arg(env!("CARGO_BIN_EXE_agent-collaboration"))
        .args([
            "conversation",
            "create",
            "--machine",
            TARGET,
            "--endpoint",
            "claude-local",
            "--cwd",
            "/tmp/on-B",
            "--access",
            "workspace-write",
            "--operation-id",
            REQUEST,
            "--from",
            &creator,
            "--text",
            "first input",
            "--json",
            "--service-directory",
        ])
        .arg(api.directory())
        .output()
        .await
        .unwrap();
        let wanted_exit = if expected["kind"] == "refused" { 2 } else { 0 };
        assert_eq!(
            output.status.code(),
            Some(wanted_exit),
            "stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(
            output
                .stdout
                .split(|byte| *byte == b'\n')
                .rfind(|line| !line.is_empty())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(value, expected);
        let calls = served.await.unwrap().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["tool"], "conversation_create");
        assert_eq!(calls[0]["arguments"]["machine"], TARGET);
        assert_eq!(calls[0]["arguments"]["operationId"], REQUEST);
        assert_eq!(calls[0]["arguments"]["firstInput"], "first input");
    }
}

#[tokio::test]
async fn remote_status_cli_renders_each_stage_and_completed_result() {
    for expected in [
        json!({"kind":"admitted"}),
        json!({"kind":"atTarget"}),
        json!({"kind":"completed","result":{"kind":"unknown"}}),
        json!({"kind":"expired"}),
    ] {
        let mut api = FakeCollaborationApi::new(SERVICE, REQUEST).unwrap();
        let served = api.serve(vec![FakeReply::Result(expected.clone())]);
        let output = run_cli(
            api.directory(),
            vec![
                "conversation".into(),
                "remote-status".into(),
                "--request-id".into(),
                REQUEST.into(),
                "--json".into(),
            ],
        )
        .await;
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).unwrap(),
            expected
        );
        let calls = served.await.unwrap().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["tool"], "conversation_remote_status");
        assert_eq!(calls[0]["arguments"], json!({"requestId":REQUEST}));
    }
}

fn arguments(actor: Value) -> Vec<String> {
    vec![
        "conversation".into(),
        "create".into(),
        "--machine".into(),
        TARGET.into(),
        "--endpoint".into(),
        "claude-local".into(),
        "--cwd".into(),
        "/tmp/on-B".into(),
        "--access".into(),
        "workspace-write".into(),
        "--from".into(),
        actor.to_string(),
        "--json".into(),
    ]
}

#[expect(
    clippy::expect_used,
    reason = "Invalid test setup must fail immediately with the failed boundary named."
)]
async fn run_cli(root: &std::path::Path, args: Vec<String>) -> std::process::Output {
    let mut command = tokio::process::Command::new(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/cargo_debug_signing_runner.sh"),
    );
    command
        .arg(env!("CARGO_BIN_EXE_agent-collaboration"))
        .args(args)
        .arg("--service-directory")
        .arg(root)
        .kill_on_drop(true);
    tokio::time::timeout(std::time::Duration::from_secs(30), command.output())
        .await
        .expect("signed CLI must complete within the test deadline")
        .expect("signed CLI must launch")
}

#[tokio::test]
async fn cli_mints_one_id_and_preserves_text_file_and_request_across_fleet_retry() {
    use collaboration_contracts::RemoteCreateFleetError;
    use collaboration_contracts::remote_create_test_support::{
        RemoteCreateStep, ScriptedRemoteCreateFleet,
    };
    use collaboration_mcp::test_support::ServedCollaborationApi;
    use std::{os::unix::fs::PermissionsExt as _, sync::Arc};
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let port = Arc::new(ScriptedRemoteCreateFleet::new(
        vec![
            RemoteCreateStep::Fail(RemoteCreateFleetError::AnswerLost),
            RemoteCreateStep::Replayed(serde_json::from_value(json!({"kind":"atTarget"})).unwrap()),
        ],
        vec![],
    ));
    let api = ServedCollaborationApi::start(
        directory.path(),
        collaboration_service::ApplicationFixture::new(SERVICE, REQUEST)
            .unwrap()
            .with_remote_create_fleet(port.clone())
            .build(),
    )
    .unwrap();
    let file = directory.path().join("first-input.txt");
    std::fs::write(&file, "from file\nsecond line").unwrap();
    let mut args = arguments(
        json!({"endpoint":{"serviceId":SERVICE,"endpointId":"claude-local"},"sessionId":"requester"}),
    );
    args.extend(["--text-file".into(), file.to_str().unwrap().into()]);
    let output = run_cli(directory.path(), args).await;
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let lines: Vec<Value> = output
        .stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect();
    let requests = port.requests().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0], requests[1]);
    assert_eq!(lines[0]["requestId"], requests[0].request_id.as_str());
    assert_eq!(lines[1]["requestId"], requests[0].request_id.as_str());
    assert_eq!(
        requests[0].command.first_input.as_ref().unwrap().as_str(),
        "from file\nsecond line"
    );
    assert!(!directory.path().join("provider-operations.sqlite").exists());
    api.stop().await.unwrap();
}

#[tokio::test]
async fn p10_cli_foreign_requester_and_target_local_actor_flags_send_nothing() {
    use collaboration_contracts::remote_create_test_support::{
        RemoteCreateStep, ScriptedRemoteCreateFleet,
    };
    use collaboration_mcp::test_support::ServedCollaborationApi;
    use std::{os::unix::fs::PermissionsExt as _, sync::Arc};
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let port = Arc::new(ScriptedRemoteCreateFleet::new(
        vec![RemoteCreateStep::Accepted],
        vec![],
    ));
    let api = ServedCollaborationApi::start(
        directory.path(),
        collaboration_service::ApplicationFixture::new(SERVICE, REQUEST)
            .unwrap()
            .with_remote_create_fleet(port.clone())
            .build(),
    )
    .unwrap();
    let local = json!({"endpoint":{"serviceId":SERVICE,"endpointId":"claude-local"},"sessionId":"requester"});
    let mut foreign = local.clone();
    foreign["endpoint"]["serviceId"] = json!(TARGET);
    let output = run_cli(directory.path(), arguments(foreign)).await;
    assert_eq!(output.status.code(), Some(2));
    assert!(port.requests().unwrap().is_empty());
    let invented = json!({"kind":"remoteRequester","machine":TARGET});
    assert_eq!(
        run_cli(directory.path(), arguments(invented))
            .await
            .status
            .code(),
        Some(2)
    );
    assert!(port.requests().unwrap().is_empty());
    let mut forced = arguments(local.clone());
    forced.extend(["--approver".into(), local.to_string()]);
    assert_eq!(
        run_cli(directory.path(), forced).await.status.code(),
        Some(2)
    );
    assert!(port.requests().unwrap().is_empty());
    let mut machine_address = arguments(local.clone());
    let machine_index = machine_address
        .iter()
        .position(|value| value == TARGET)
        .unwrap();
    machine_address[machine_index] = json!({"machine":TARGET}).to_string();
    assert_eq!(
        run_cli(directory.path(), machine_address)
            .await
            .status
            .code(),
        Some(2)
    );
    assert!(port.requests().unwrap().is_empty());
    let ordinary = vec![
        "message".into(),
        "send".into(),
        "--to".into(),
        json!({"machine":TARGET}).to_string(),
        "--from".into(),
        local.to_string(),
        "--text".into(),
        "forged target".into(),
        "--json".into(),
    ];
    let rejected = run_cli(directory.path(), ordinary).await;
    assert_eq!(
        rejected.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&rejected.stdout)
    );
    assert!(port.requests().unwrap().is_empty());
    assert_eq!(
        run_cli(directory.path(), arguments(local))
            .await
            .status
            .code(),
        Some(0)
    );
    assert_eq!(
        port.requests().unwrap().len(),
        1,
        "valid control reaches the same stand-in"
    );
    api.stop().await.unwrap();
}
