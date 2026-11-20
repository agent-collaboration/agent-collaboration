//! N1 node taps on the real connection loop: a Codex CLI reply to a server request travels to
//! the approvals broker (N7), and a client notification without an id is ignored. Each case
//! records the whole connection from its first frame.
use super::node_tap_tests::wire::CliConnectionRecorder;
use super::node_tap_tests::{commands_seen_at_n4, new_context, state_with_commands};
use super::test_support::ScriptedSessionBackend;
use super::*;
use codex_router_test_support::node_taps::{
    CatalogRowExpectation, EdgeParty, MessageFamily, ProtocolNode, RecordIntent, TapCase,
};
use tokio_tungstenite::client_async;

#[tokio::test]
async fn node_tap_n1_ignored_client_notification_without_id() {
    // Arrange: a live connection loop over an in-process socket pair.
    let backend = ScriptedSessionBackend::new().expect("backend");
    let (client, server) = UnixStream::pair().expect("pair");
    let task = tokio::spawn(serve_router_session_app_server_connection(
        server,
        new_context(&backend),
    ));
    let (mut client, _) = client_async("ws://localhost/rpc", client)
        .await
        .expect("client");
    let mut case = TapCase::new(
        ProtocolNode::N1CliFace,
        "ignored_client_notification_without_id",
        "Dispositions: Ignored ClientNotificationWithoutId",
    );
    case.state_before(state_with_commands(0));
    let mut connection = CliConnectionRecorder::default();
    // Act: a notification with no id, then an orderly close; every frame N1 sends is recorded.
    connection
        .send(&mut case, &mut client, json!({"method":"initialized"}))
        .await;
    client.close(None).await.expect("close");
    while connection
        .next_frame(&mut case, &mut client)
        .await
        .is_some()
    {}
    task.await.expect("join").expect("connection ended cleanly");
    case.state_after(state_with_commands(commands_seen_at_n4(&backend).len()));
    // Assert: nothing is answered and no command is issued.
    let row = CatalogRowExpectation::new().state_after(state_with_commands(0));
    case.finish(&row).assert_settled();
}

#[tokio::test]
async fn node_tap_n1_connection_refused_config_write() {
    // Arrange: a live connection loop over an in-process socket pair.
    let backend = ScriptedSessionBackend::new().expect("backend");
    let (client, server) = UnixStream::pair().expect("pair");
    let task = tokio::spawn(serve_router_session_app_server_connection(
        server,
        new_context(&backend),
    ));
    let (mut client, _) = client_async("ws://localhost/rpc", client)
        .await
        .expect("client");
    let mut case = TapCase::new(
        ProtocolNode::N1CliFace,
        "connection_refused_config_write",
        "Dispositions: Refused ConfigWrite (config/write)",
    );
    case.state_before(state_with_commands(0));
    let mut connection = CliConnectionRecorder::default();
    // Act: the test, playing the CLI, sends `config/write` on the wire; every frame N1 sends is
    // recorded until the connection ends. The pinned 0.166.0 CLI never sends `config/write`, so the
    // test's own frame is apparatus: its method violation is not a Router defect. The Router defect
    // under it (N1's method table names `config/write` and misses `config/value/write`) is an
    // inventory row (router_session_app_server_methods.rs:87, D1).
    connection
        .send(
            &mut case,
            &mut client,
            json!({"id":5,"method":"config/write","params":{"keyPath":"model","value":"x","mergeStrategy":"replace"}}),
        )
        .await;
    case.mark_last(RecordIntent::SyntheticApparatus);
    let answer = connection
        .next_frame(&mut case, &mut client)
        .await
        .expect("config/write answer");
    client.close(None).await.expect("close");
    while connection
        .next_frame(&mut case, &mut client)
        .await
        .is_some()
    {}
    task.await.expect("join").expect("connection ended cleanly");
    case.state_after(state_with_commands(commands_seen_at_n4(&backend).len()));
    // Assert: a visible -32000 refusal, no session command.
    assert!(answer.get("result").is_none(), "{answer}");
    let row = CatalogRowExpectation::new()
        .produces_with(
            MessageFamily::CodexClientRequestFromCli,
            EdgeParty::CodexCli,
            "error:-32000:config/write",
            json!({"id":5,"error":{"code":-32000,"message":"Saving Codex configuration/defaults is not supported on this ACP endpoint"}}),
        )
        .state_after(state_with_commands(0));
    case.finish(&row).assert_settled();
}
