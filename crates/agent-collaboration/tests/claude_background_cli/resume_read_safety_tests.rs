//! A conflict belongs to its target; missing live evidence never proves absence.
use super::*;
use agent_session_inventory::sessions::{
    source_client_action::{CapturedClientAction, SessionClientEffect, run_local_claude_terminal},
    source_ssh_protocol::{SourceOperation, SourceRequest},
};
use agent_session_inventory::source_inventory_request::PickerSourceContext;
use std::process::Stdio;

const CONFLICTED_SESSION: &str = "00000000-0000-4000-8000-000000000043";

fn resume_request(fixture: &ClaudeProcessFixture, session_id: &str) -> serde_json::Value {
    json!({
        "serviceDirectory":fixture.root.path().join("router/agent-communication"),
        "expectedService":"00000000-0000-4000-8000-000000000001",
        "operation":{"kind":"nativeClaude","serviceEpoch":"00000000-0000-4000-8000-000000000002",
            "action":{"kind":"resume","target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},"sessionId":session_id},
                "directory":fixture.working_directory,"arguments":[]}}
    })
}

fn picker_command(fixture: &ClaudeProcessFixture, session_id: &str) -> std::io::Result<Command> {
    let template = attach_command(fixture);
    let mut command = Command::new(std::env::current_exe()?);
    command
        .kill_on_drop(true)
        .current_dir(&fixture.working_directory)
        .args([
            "--exact",
            "attach_flow_tests::resume_read_safety_tests::picker_terminal_preparation_child",
            "--ignored",
            "--nocapture",
        ])
        .env(
            "CLAUDE_PICKER_REQUEST",
            resume_request(fixture, session_id).to_string(),
        );
    for (name, value) in template.as_std().get_envs() {
        if let Some(value) = value {
            command.env(name, value);
        } else {
            command.env_remove(name);
        }
    }
    Ok(command)
}

#[test]
#[ignore = "parent runs this probe in a child with a fixture-local PATH"]
fn picker_terminal_preparation_child() -> Result<(), Box<dyn std::error::Error>> {
    let request: SourceRequest = serde_json::from_str(&std::env::var("CLAUDE_PICKER_REQUEST")?)?;
    let SourceOperation::NativeClaude {
        service_epoch,
        action,
    } = request.operation
    else {
        return Err("probe requires a native Claude action".into());
    };
    run_local_claude_terminal(
        CapturedClientAction {
            attempt: 1,
            source: PickerSourceContext::DefaultHosted,
            service_id: request.expected_service,
            effect: SessionClientEffect::Native {
                service_epoch,
                action,
            },
        },
        std::path::Path::new(request.service_directory.as_str()),
        None,
        &[],
    )?;
    Ok(())
}

fn write_live_peer(fixture: &ClaudeProcessFixture, pid: u32) -> std::io::Result<()> {
    let peers = fixture.root.path().join("peers");
    fs::write(
        peers.join(format!("{pid}.json")),
        serde_json::to_vec(&json!({
            "pid":pid,"sessionId":CONFLICTED_SESSION,"cwd":fixture.working_directory,
            "status":"idle","startedAt":1790162100000_i64,"updatedAt":1790162101000_i64,
            "statusUpdatedAt":1790162101000_i64,"kind":"interactive","entrypoint":"cli",
            "peerProtocol":1,"messagingSocketPath":peers.join("peer.sock")
        }))?,
    )
}

#[tokio::test]
async fn unrelated_live_conflict_does_not_block_stopped_cli_or_picker_open() {
    let fixture = ClaudeProcessFixture::new().await.unwrap();
    let api = serve_job(&fixture, "stopped").unwrap();
    let mut second_peer = Command::new("/bin/cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    write_live_peer(&fixture, std::process::id()).unwrap();
    write_live_peer(&fixture, second_peer.id().unwrap()).unwrap();
    for mut command in [
        {
            let mut command = attach_command(&fixture);
            command.args(["--provider", "claude", "--id", SESSION]);
            command
        },
        picker_command(&fixture, SESSION).unwrap(),
    ] {
        let before = invocations(&fixture).unwrap().len();
        let output = command.output().await.unwrap();
        assert!(
            output.status.success(),
            "healthy stopped A must open despite B's conflict: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let calls = invocations(&fixture).unwrap();
        assert_eq!(calls.len(), before + 2);
        assert_eq!(calls[before].arguments, ["--bg", "--resume", SESSION]);
        assert_eq!(calls[before + 1].arguments, ["attach", "a1b2c3d4"]);
    }
    for mut command in [
        {
            let mut command = attach_command(&fixture);
            command.args(["--provider", "claude", "--id", CONFLICTED_SESSION]);
            command
        },
        picker_command(&fixture, CONFLICTED_SESSION).unwrap(),
    ] {
        let before = invocations(&fixture).unwrap().len();
        let output = command.output().await.unwrap();
        assert!(!output.status.success(), "conflicted B must not open");
        assert_eq!(
            invocations(&fixture).unwrap().len(),
            before,
            "B must launch no vendor child"
        );
    }
    second_peer.kill().await.unwrap();
    second_peer.wait().await.unwrap();
    api.stop().await.unwrap();
}

#[tokio::test]
async fn unavailable_peers_cannot_resume_a_known_stopped_job() {
    let fixture = ClaudeProcessFixture::new().await.unwrap();
    let api = serve_job(&fixture, "stopped").unwrap();
    fs::rename(
        fixture.root.path().join("peers"),
        fixture.root.path().join("peers-retained"),
    )
    .unwrap();
    fs::write(
        fixture.root.path().join("peers"),
        b"not a readable peer directory",
    )
    .unwrap();
    let mut background = attach_command(&fixture);
    background.args([
        "--source-request-v1",
        &resume_request(&fixture, SESSION).to_string(),
    ]);
    let mut direct = attach_command(&fixture);
    direct.args(["--provider", "claude", "--id", SESSION]);
    for mut command in [
        background,
        direct,
        picker_command(&fixture, SESSION).unwrap(),
    ] {
        let output = command.output().await.unwrap();
        assert!(
            !output.status.success(),
            "missing peers cannot prove the stopped job is safe to resume"
        );
        assert!(
            invocations(&fixture).unwrap().is_empty(),
            "unconfirmed peers must launch no vendor child"
        );
    }
    api.stop().await.unwrap();
}

#[tokio::test]
async fn unavailable_loaded_read_cannot_resume_a_known_stopped_job() {
    use agent_session_inventory::test_support::fake_api_support::{
        FakeCollaborationApi, FakeReply,
    };
    let fixture = ClaudeProcessFixture::new().await.unwrap();
    let mut api = FakeCollaborationApi::new(
        "00000000-0000-4000-8000-000000000001",
        "00000000-0000-4000-8000-000000000002",
    )
    .unwrap();
    fs::rename(
        api.directory().join("control.sock"),
        fixture
            .root
            .path()
            .join("router/agent-communication/control.sock"),
    )
    .unwrap();
    let serving = api.serve(vec![
        FakeReply::Result(json!({"serviceEpoch":"00000000-0000-4000-8000-000000000002","sequence":1,"endpoints":[{
            "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},"label":"Native Claude","channels":[],"availability":{"state":"unavailable","observedAt":"2026-10-18T00:00:00Z","reason":"ACP unavailable"}}]})),
        FakeReply::Error(json!({"code":-32050,"message":"Provider session inventory unavailable","data":{"kind":"unavailable","stage":"discovery","message":"Provider session inventory unavailable"}})),
        FakeReply::Result(json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},"observedAt":"2026-10-18T00:00:00Z","nativeCoverage":{"peer":{"kind":"notRequested"},"background":{"kind":"complete"}},"sessions":[{
            "origin":"claudeCodeBackground","target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},"sessionId":SESSION},"workingDirectory":fixture.working_directory,
            "jobHandle":"deadbeef","startedAt":1790162100,"state":{"kind":"stopped"}}]})),
    ]);
    let output = attach_command(&fixture)
        .args([
            "--source-request-v1",
            &resume_request(&fixture, SESSION).to_string(),
        ])
        .output()
        .await
        .unwrap();
    serving.abort();
    let _ = serving.await;
    assert!(
        !output.status.success(),
        "a failed Loaded read cannot establish absence"
    );
    assert!(
        invocations(&fixture).unwrap().is_empty(),
        "unavailable Loaded must launch no vendor child"
    );
}

#[tokio::test]
async fn explicit_fork_keeps_its_existing_policy_when_peers_are_unavailable() {
    let fixture = ClaudeProcessFixture::new().await.unwrap();
    let api = serve_job(&fixture, "stopped").unwrap();
    fs::rename(
        fixture.root.path().join("peers"),
        fixture.root.path().join("peers-retained"),
    )
    .unwrap();
    fs::write(fixture.root.path().join("peers"), b"unavailable peers").unwrap();
    let mut request = resume_request(&fixture, SESSION);
    request["operation"]["action"]["kind"] = json!("fork");
    let output = attach_command(&fixture)
        .args(["--source-request-v1", &request.to_string()])
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "explicit Fork remains allowed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let calls = invocations(&fixture).unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].arguments,
        ["--bg", "--resume", SESSION, "--fork-session"]
    );
    api.stop().await.unwrap();
}
