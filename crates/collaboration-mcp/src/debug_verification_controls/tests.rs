//! Real owner-socket HTTP calls exercise control admission and causal release.
use super::DebugVerificationControls;
use crate::{CollaborationApiListener, collaboration_api_router, serve_collaboration_api};
use collaboration_runtime::{
    AutomationFaultPoint, AutomationFaultSelector, DebugAutomationFaultController, OwnerOnlySocket,
};
use serde_json::{Value, json};
use std::{io, path::Path, sync::Arc, time::Duration};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

async fn post_control(socket: &Path, action: &str, request: Value) -> (u16, Value) {
    let mut stream = tokio::net::UnixStream::connect(socket)
        .await
        .expect("connect the owned API socket");
    let body = serde_json::to_vec(&request).expect("control request JSON");
    let header = format!(
        "POST /debug/automation/faults/{action} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    stream
        .write_all(header.as_bytes())
        .await
        .expect("request header");
    stream.write_all(&body).await.expect("request body");
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), stream.read_to_end(&mut response))
        .await
        .expect("control response completion guard")
        .expect("read control response");
    let response = String::from_utf8(response).expect("HTTP response UTF-8");
    let (header, body) = response
        .split_once("\r\n\r\n")
        .expect("HTTP response boundary");
    let status = header
        .split_whitespace()
        .nth(1)
        .expect("HTTP status")
        .parse()
        .expect("numeric HTTP status");
    let body = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(body).expect("control response JSON")
    };
    (status, body)
}

#[test]
fn debug_controls_refuse_a_tcp_api_copy() {
    let directory = tempfile::tempdir().expect("fixture directory");
    let config = crate::api_test_harness::api_config(
        crate::api_test_harness::test_identity().build(),
        directory.path(),
    );
    let api = collaboration_api_router(
        &config,
        CollaborationApiListener::LoopbackTcp {
            address: ([127, 0, 0, 1], 12345).into(),
            proxy_authority: None,
        },
    );
    let error = api
        .with_debug_verification_controls(DebugVerificationControls::default())
        .err()
        .expect("debug controls must refuse TCP even with no controllers");
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
}

#[tokio::test]
async fn owner_socket_controls_hold_until_the_matching_release() {
    use std::os::unix::fs::PermissionsExt as _;
    // Arrange: the real guarded API on an owner-only Unix socket, with a real controller.
    let directory = tempfile::tempdir_in("/tmp").expect("short owned socket directory");
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
        .expect("private socket directory");
    let socket = directory.path().join("control.sock");
    let (listener, _cleanup) = OwnerOnlySocket::bind(&socket)
        .expect("bind owned socket")
        .into_parts();
    let config = crate::api_test_harness::api_config(
        crate::api_test_harness::test_identity().build(),
        directory.path(),
    );
    let faults = Arc::new(DebugAutomationFaultController::default());
    let api = collaboration_api_router(&config, CollaborationApiListener::UnixSocket)
        .with_debug_verification_controls(DebugVerificationControls {
            automation_faults: Some(Arc::clone(&faults)),
        })
        .expect("owner socket admits named controls");
    let serving = tokio::spawn(serve_collaboration_api(
        listener,
        api,
        config.shutdown.clone(),
    ));

    // Act: arm over HTTP, then reach the actual controller hold with matching evidence.
    let (status, arm) = post_control(
        &socket,
        "arm",
        json!({
            "point":"entryAdmissionCommitted", "selector":{"completedEntryOrdinal":2}
        }),
    )
    .await;
    assert_eq!(status, 200);
    let arm_id: collaboration_protocol::OperationId =
        serde_json::from_value(arm["armId"].clone()).expect("real arm identity");
    let held_faults = Arc::clone(&faults);
    let held = tokio::spawn(async move {
        use automation_storage::AutomationFaultObserver as _;
        held_faults
            .hold(
                AutomationFaultPoint::EntryAdmissionCommitted,
                automation_storage::AutomationFaultEvidence {
                    completed_entry_ordinal: Some(2),
                    ..Default::default()
                },
            )
            .await;
    });
    let (status, observed) = post_control(&socket, "wait", arm.clone()).await;
    assert_eq!(status, 200);
    assert_eq!(observed["armId"], arm["armId"]);
    assert_eq!(observed["point"], "entryAdmissionCommitted");
    assert_eq!(observed["evidence"]["completedEntryOrdinal"], 2);
    assert!(
        !held.is_finished(),
        "hold ended before its matching release"
    );
    let (status, _) = post_control(
        &socket,
        "release",
        json!({
            "armId":collaboration_protocol::OperationId::generate()
        }),
    )
    .await;
    assert_eq!(status, 409);
    assert!(!held.is_finished(), "a foreign release ended the hold");
    let (status, _) = post_control(&socket, "release", arm).await;
    assert_eq!(status, 204);
    tokio::time::timeout(Duration::from_secs(3), held)
        .await
        .expect("release completion guard")
        .expect("held task");

    // Assert: the existing listener lifecycle still retires after control calls finish.
    config.shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(3), serving)
        .await
        .expect("API retirement guard")
        .expect("API task")
        .expect("API stop");
    assert!(faults.release(&arm_id).await.is_err());
    assert!(
        faults
            .arm(
                AutomationFaultPoint::BatchFetchedBeforeAdmission,
                AutomationFaultSelector::default()
            )
            .await
            .is_ok()
    );
}
