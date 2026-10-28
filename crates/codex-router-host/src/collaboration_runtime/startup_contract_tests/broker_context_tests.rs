use super::{BOUND, start_host, target};
use collaboration_protocol::{PushHeaderFacts, PushKind, RouterLink};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::os::unix::fs::PermissionsExt as _;
use tokio::io::{AsyncBufReadExt as _, BufReader};

#[tokio::test]
async fn host_broker_notices_use_the_shared_display_names_and_physical_push_store() {
    let fixture = start_host(false).await;
    let requester = target(&fixture.runtime, "named-requester");
    let approver = target(&fixture.runtime, "approver");
    fixture
        .runtime
        .startup_test_display_names
        .remember(requester.clone(), "Host requester");
    let registry = fixture.directory.path().join("peers");
    let socket = registry.join("peer.sock");
    let listener = tokio::net::UnixListener::bind(&socket).expect("owned peer listener");
    let process = std::process::id();
    std::fs::write(
        registry.join(format!("{process}.json")),
        json!({
            "pid":process,"sessionId":approver.session_id,"status":"busy","peerProtocol":1,
            "messagingSocketPath":socket
        })
        .to_string(),
    )
    .expect("peer record");
    let digest = Sha256::digest(socket.to_str().expect("socket path").as_bytes());
    let key = registry.join(format!("{process}.{digest:x}.key"));
    std::fs::write(&key, r#"{"peerToken":"0123456789abcdef0123456789abcdef"}"#)
        .expect("test peer token");
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).expect("private token");
    let broker = fixture
        .runtime
        .startup_test_broker
        .upgrade()
        .expect("actual Host broker");
    let physical = broker.interaction_store();
    let requester = serde_json::from_value(serde_json::to_value(&requester).expect("requester"))
        .expect("board requester");
    let approver: message_board::SessionRef =
        serde_json::from_value(serde_json::to_value(&approver).expect("approver"))
            .expect("board approver");
    let response = broker.request_question(requester, message_board::Identity::Session { session: approver },
        serde_json::from_value(json!({
            "requestId":"startup-question","prompt":"Choose?","fields":[
                {"kind":"text","fieldId":"answer","label":"Answer","description":null,"required":true}
            ]
        })).expect("typed question"),None).await.expect("question registered");
    let notice = tokio::time::timeout(BOUND, async {
        let (stream, _) = listener.accept().await.expect("actual broker route");
        let mut lines = BufReader::new(stream).lines();
        let auth: Value = serde_json::from_str(
            &lines
                .next_line()
                .await
                .expect("auth IO")
                .expect("auth frame"),
        )
        .expect("auth JSON");
        assert_eq!(auth["type"], "auth");
        serde_json::from_str::<Value>(
            &lines
                .next_line()
                .await
                .expect("notice IO")
                .expect("notice frame"),
        )
        .expect("notice JSON")
    })
    .await
    .expect("Host broker must publish through its installed push context");
    let content = notice["message"]["content"]
        .as_str()
        .expect("consumer notice content");
    let link = content
        .split_whitespace()
        .find(|word| word.starts_with("router://"))
        .expect("stored push link");
    let link = RouterLink::parse(link).expect("canonical link");
    let record = physical
        .get_push_record(link.push_id())
        .await
        .expect("physical store read")
        .expect("notice was committed to the Host broker's physical store before delivery");
    assert_eq!(record.kind, PushKind::Question);
    assert!(
        matches!(record.header_facts,PushHeaderFacts::Question { requester_display_name:Some(ref name),.. } if name.as_str()=="Host requester"),
        "broker must use the same display-name cache populated by Host operations"
    );
    assert!(
        record.origin_router_ref.is_some(),
        "typed notice retains its originating interaction"
    );
    drop(response);
    drop(physical);
    drop(broker);
    fixture.runtime.shutdown().await.expect("Host shutdown");
}
