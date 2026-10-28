//! SSH executable substitution proves the carrier boundary only. Native child
//! receipt processing and actual two-machine/vendor behavior have separate gates.
use super::*;
use crate::sessions::router_connection_registry::RouterConnectionRegistry;
use collaboration_client::protocol::SessionRef;
use serde_json::json;
use std::{ffi::OsString, os::unix::fs::PermissionsExt};

const CHILD_TEST: &str = "sessions::source_action_transport::tests::native_ssh_receipt_probe";

fn profile() -> RouterConnectionProfile {
    RouterConnectionRegistry::parse(
        &json!({"version":1,"routers":[{
        "name":"Captured source","connection":{"kind":"ssh",
        "serviceId":"00000000-0000-4000-8000-000000000001",
        "destination":"source.example.invalid","executable":"/fixed/agent-sessions",
        "serviceDirectory":"/private/source's root","clientProfile":"routerDebug"},
        "defaultRemoteCwd":"/source/project"}]})
        .to_string(),
    )
    .unwrap()
    .routers
    .remove(0)
}

#[tokio::test]
async fn native_ssh_lost_or_misattributed_receipt_keeps_action_unknown_without_replay() {
    let root = tempfile::Builder::new()
        .prefix("native-ssh-")
        .tempdir()
        .unwrap();
    let executable = root.path().join("ssh");
    codex_router_test_support::node_fixture::write_node_fixture(
        &executable,
        "source-ssh-agent.ts",
        &json!({"kind":"receipt"}),
    )
    .unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    for mode in [
        "lost",
        "wrong",
        "observed",
        "old",
        "managed-lost",
        "managed-wrong",
        "managed-unknown",
        "managed-completed",
        "managed-wrong-id",
        "managed-old",
    ] {
        let count = root.path().join(format!("{mode}.count"));
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            tokio::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", CHILD_TEST, "--ignored", "--nocapture"])
                .env("PATH", format!("{}:/usr/bin:/bin", root.path().display()))
                .env("SOURCE_SSH_COUNT", &count)
                .env("SOURCE_SSH_MODE", mode)
                .kill_on_drop(true)
                .output(),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(
            output.status.success(),
            "{mode}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            std::fs::read_to_string(count).unwrap().lines().count(),
            1,
            "{mode}: never replay uncertain native submission"
        );
    }
}

#[tokio::test]
#[ignore = "isolated PATH/executable transport probe, invoked by its parent"]
async fn native_ssh_receipt_probe() {
    let mode = std::env::var("SOURCE_SSH_MODE").unwrap();
    if mode.starts_with("managed-") {
        managed_ssh_receipt_probe(&mode).await;
        return;
    }
    let action = SourceNativeClaudeAction::New {
        directory: "/source/project".to_owned().try_into().unwrap(),
        arguments: vec![OsString::from("--name"), OsString::from("literal value")],
    };
    let expected = action.captured_identity();
    let result = submit_native_source(
        &profile(),
        "00000000-0000-4000-8000-000000000002"
            .to_owned()
            .try_into()
            .unwrap(),
        action,
    )
    .await;
    if mode == "old" {
        assert!(matches!(
            result,
            Err(SourceReadError::Rejected(SourceRejection::InvalidRequest))
        ));
        return;
    }
    let result = result.unwrap();
    assert_eq!(result.action, expected);
    assert_eq!(result.source.service_id, profile().service_id);
    match result.observation {
        SourceNativeObservation::Observed { stdout, .. } if mode == "observed" => {
            assert_eq!(stdout, b"opaque-native-handle\n")
        }
        SourceNativeObservation::Unknown {
            reason: SourceNativeUnknownReason::TransportUnavailable,
            ..
        } if mode == "lost" || mode == "wrong" => {}
        other => panic!("{mode}: receipt must retain observed/unknown distinction: {other:?}"),
    }
    let wire = serde_json::to_string(&result.action).unwrap();
    assert!(!wire.contains("operationId") && !wire.contains("operationShow"));
}

async fn managed_ssh_receipt_probe(mode: &str) {
    let profile = profile();
    let binding: ProviderBindingIdentity = serde_json::from_value(json!({
        "endpoint":{"serviceId":profile.service_id,"endpointId":"cursor-source"},
        "bindingId":"actual-binding","runtime":{"provider":"cursor","runtimeName":"fixture"},
        "transport":"stdioAcp","generation":{"serviceEpoch":"00000000-0000-4000-8000-000000000002","generation":4},
        "capabilities":[{"name":"load","status":"supported","evidence":"advertised"}]
    })).unwrap();
    let target: SessionRef = serde_json::from_value(
        json!({"endpoint":binding.endpoint,"sessionId":"captured-managed-target"}),
    )
    .unwrap();
    let operation_id = OperationId::generate();
    let result = submit_managed_source(
        &profile,
        operation_id.clone(),
        binding,
        SourceManagedAction::Load {
            target: target.clone(),
        },
    )
    .await;
    if mode == "managed-old" {
        assert!(matches!(
            result,
            Err(SourceReadError::Rejected(SourceRejection::InvalidRequest))
        ));
    } else if mode == "managed-completed" {
        let SourceProviderActionResult::ManagedLoad { result } = result.unwrap() else {
            panic!("valid completed managed receipt must be accepted")
        };
        assert!(
            matches!(*result,collaboration_client::ConversationOperationResult::Completed {target:actual,operation_id:Some(actual_id),..} if actual==target && actual_id==operation_id)
        );
    } else {
        assert!(
            matches!(result.unwrap(),SourceProviderActionResult::Unknown {operation_id:actual,target:Some(actual_target)} if actual == operation_id && actual_target == target),
            "retain caller identity without adopting wrong reply or resubmitting"
        );
    }
}
