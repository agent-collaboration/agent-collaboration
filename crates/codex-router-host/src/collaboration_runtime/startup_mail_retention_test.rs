//! The actual Host consumes clock controls and runs library-owned custody removal.
use super::{BOUND, client, start_host_in, target};
use chrono::{DateTime, Utc};
use collaboration_client::MessageSendRequest;
use collaboration_protocol::{MailItemId, MessageSendResult};
use serde_json::{Value, json};
use sqlx::Connection as _;
use std::os::unix::fs::PermissionsExt as _;
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

fn item_at(now: DateTime<Utc>) -> MailItemId {
    let timestamp = uuid::Timestamp::from_unix(
        uuid::NoContext,
        u64::try_from(now.timestamp()).expect("positive timestamp"),
        now.timestamp_subsec_nanos(),
    );
    uuid::Uuid::new_v7(timestamp)
        .to_string()
        .try_into()
        .expect("UUIDv7 mail identity")
}

async fn custody(
    connection: &mut sqlx_turso::TursoConnection,
    outbox: &MailItemId,
    inbox: &MailItemId,
) -> (i64, i64) {
    sqlx::query_as("SELECT (SELECT count(*) FROM _fleet_sync_outbox_items WHERE item_id=?), (SELECT count(*) FROM _fleet_sync_inbox_items WHERE item_id=?)")
        .bind(outbox.as_str())
        .bind(inbox.as_str())
        .fetch_one(connection)
        .await
        .expect("independent physical custody read")
}

#[tokio::test]
async fn host_mail_clock_steps_remove_both_holders_at_day_60_and_refuse_day_61() {
    // Arrange: a private control socket, deterministic UUIDv7 times, actual Host and API.
    let now: DateTime<Utc> = "2026-11-04T00:00:00Z".parse().expect("fixture time");
    let directory = tempfile::Builder::new()
        .prefix("mail-clock-")
        .tempdir_in("/tmp")
        .expect("private root");
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
        .expect("private permissions");
    let socket = directory.path().join("clock.sock");
    let listener = tokio::net::UnixListener::bind(&socket).expect("control listener");
    let input = directory.path().join("messaging-test-mail-clock.json");
    std::fs::write(
        &input,
        json!({"now":now,"controlSocket":socket}).to_string(),
    )
    .expect("clock input");
    std::fs::set_permissions(input, std::fs::Permissions::from_mode(0o600)).expect("private input");
    let fixture = start_host_in(true, directory).await;
    let (stream, _) = tokio::time::timeout(BOUND, listener.accept())
        .await
        .expect("Host must attach its debug mail clock control")
        .expect("control connection");
    let mut control = BufReader::new(stream);
    let mut response = String::new();
    tokio::time::timeout(BOUND, control.read_line(&mut response))
        .await
        .expect("clock ready bound")
        .expect("clock ready");
    assert_eq!(
        serde_json::from_str::<Value>(&response).unwrap(),
        json!({"kind":"ready","now":now})
    );
    let api = client(&fixture).await;
    let local = target(&fixture.runtime, "retained-local-inbox");
    let mut remote = target(&fixture.runtime, "retained-remote-outbox");
    let remote_service = uuid::Uuid::now_v7().to_string();
    remote.endpoint.service_id = remote_service.clone().try_into().expect("remote service");
    let broker = fixture
        .runtime
        .startup_test_broker
        .upgrade()
        .expect("Host broker");
    let physical = broker.interaction_store();
    physical
        .remember_installation_peer(&interaction_storage::InstallationPeerBinding {
            service_id: remote_service.try_into().expect("remote binding service"),
            peer_id: uuid::Uuid::now_v7()
                .to_string()
                .try_into()
                .expect("remote peer"),
        })
        .await
        .expect("normal installation directory cache");
    let outbox = item_at(now);
    let inbox = item_at(now);
    for (item, recipient) in [(&outbox, &remote), (&inbox, &local)] {
        let request: MessageSendRequest = serde_json::from_value(json!({
            "itemId":item,"target":recipient,"message":{"kind":"humanUser","text":"retention boundary"},"delivery":"auto"
        })).expect("normal send request");
        let sent = api.send_message(request).await;
        if item == &outbox {
            assert!(
                matches!(sent, Ok(MessageSendResult::Queued { .. })),
                "{sent:?}"
            );
        }
        // An absent local carrier may refuse delivery; custody is still the real Inbox's.
    }
    let mut connection = sqlx_turso::TursoConnection::connect_with(
        &sqlx_turso::TursoConnectOptions::new()
            .filename(fixture.directory.path().join("interaction.sqlite")),
    )
    .await
    .expect("independent database reader");
    assert_eq!(custody(&mut connection, &outbox, &inbox).await, (1, 1));

    // Act and assert: completion replies follow the due-work pass, never a sleep.
    for (days, expected) in [(59, (1, 1)), (60, (0, 0))] {
        let stepped = now + chrono::Duration::days(days);
        control
            .get_mut()
            .write_all(format!("{}\n", json!({"now":stepped})).as_bytes())
            .await
            .expect("step request");
        response.clear();
        tokio::time::timeout(BOUND, control.read_line(&mut response))
            .await
            .expect("step completion bound")
            .expect("step completion");
        let answer: Value = serde_json::from_str(&response).expect("step reply");
        assert_eq!(answer, json!({"kind":"stepped","now":stepped}));
        assert_eq!(
            custody(&mut connection, &outbox, &inbox).await,
            expected,
            "day {days}"
        );
    }
    let too_old = item_at(now - chrono::Duration::days(1));
    let draft = collaboration_protocol::PushRecordDraft {
        push_id: too_old.as_str().to_owned().try_into().expect("old push id"),
        kind: collaboration_protocol::PushKind::DirectMessage,
        origin: collaboration_protocol::PushOrigin::OwnerUnverified,
        origin_router_ref: None,
        target: local,
        mode: Some(collaboration_protocol::MessageDelivery::Auto),
        guard: None,
        reply_to_push_id: None,
        header_facts: collaboration_protocol::PushHeaderFacts::DirectMessage {
            sender_display_name: None,
        },
        body: Some("61 days old".to_owned()),
        activity: None,
        created_at: now - chrono::Duration::days(1),
    };
    let old_item = collaboration_runtime::outgoing_mail_message(
        &draft,
        &String::from(fixture.runtime.service_id().clone())
            .try_into()
            .expect("sender service"),
        fixture.runtime.installation_peer(),
    )
    .expect("old item through the normal outgoing encoder");
    let envelope = serde_json::from_value(
        serde_json::to_value(fleet_protocol::DeliveryEnvelope::Item(old_item)).unwrap(),
    )
    .unwrap();
    let mail = fixture
        .runtime
        .startup_test_mail
        .as_ref()
        .and_then(std::sync::Weak::upgrade)
        .expect("Host mail owner");
    let refusal = mail
        .accept_pushed_mail(
            fleet_protocol::Submitter::Machine(fixture.runtime.installation_peer()),
            envelope,
        )
        .await
        .expect("normal typed incoming-mail intake");
    assert!(
        matches!(
            refusal,
            fleet_protocol::PushResult::Refused {
                reason: fleet_protocol::DeliveryRefusal::TooOld
            }
        ),
        "{refusal:?}"
    );
    assert_eq!(custody(&mut connection, &outbox, &too_old).await, (0, 0));
    connection.close().await.expect("reader close");
    drop(mail);
    drop(physical);
    drop(broker);
    drop(api);
    fixture.runtime.shutdown().await.expect("Host shutdown");
}
