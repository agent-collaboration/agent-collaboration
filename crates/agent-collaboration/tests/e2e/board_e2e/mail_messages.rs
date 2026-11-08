//! Replay and lost-caller-answer observations over actual Unix CLI/TCP MCP.
use super::mail_fixture::{MailFixture, at};
use collaboration_contracts::RouterDeliveryTypes;
use owned_process_test_support::scenario_finalization::run_scenario;
use router_peer_test_support::FixtureResult;
use serde_json::{Value, json};
use sqlx_turso::sqlx::{ConnectOptions, Connection};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub const REPLAY_TEXT: &str = "LM02 literal same identity, one actual peer write";

pub async fn cli_send(suite: &MailFixture, item: &str, text: &str) -> FixtureResult<Value> {
    suite
        .cli(&[
            "message",
            "send",
            "--human-user",
            "--to",
            &suite.target.to_string(),
            "--text",
            text,
            "--item-id",
            item,
        ])
        .await
}

pub async fn fixed_identity(suite: &mut MailFixture) -> FixtureResult {
    let replay_identity = super::mail_fixture::identity();
    let replay_id = replay_identity.as_str();
    let original = cli_send(suite, replay_id, REPLAY_TEXT).await?;
    assert_eq!(at(&original, "/pushId")?, &json!(replay_id));
    assert_eq!(
        original.pointer("/receipt/outcome/kind"),
        Some(&json!("peerMessageWritten"))
    );
    let frame = suite
        .peer_mut()?
        .expect_text_with_timeout(REPLAY_TEXT, Duration::from_secs(15))
        .await?;
    assert!(frame.contains(replay_id));
    assert_eq!(suite.peer()?.count_after_barrier().await?, 1);
    for reopening in [false, true] {
        if reopening {
            suite.stop_host().await?;
            suite.reopen_host().await?;
        }
        assert_eq!(
            suite
                .host()?
                .mcp(
                    "message_send",
                    suite.message_input(Some(replay_id), REPLAY_TEXT)
                )
                .await?,
            original
        );
        assert_eq!(cli_send(suite, replay_id, REPLAY_TEXT).await?, original);
        check_refusals(suite, &original, replay_id).await?;
        let shown = suite
            .host()?
            .mcp(
                "router_show",
                json!({"caller":suite.target,"reference":at(&original, "/link")?}),
            )
            .await?;
        assert_eq!(shown.pointer("/record/pushId"), Some(&json!(replay_id)));
        assert_eq!(shown.pointer("/record/body"), Some(&json!(REPLAY_TEXT)));
        assert_eq!(suite.peer()?.count_after_barrier().await?, 1);
    }
    suite.stop_host().await?;
    terminal_readback(suite, replay_id, &original).await?;
    assert_eq!(suite.peer()?.count_after_barrier().await?, 1);
    Ok(())
}

async fn check_refusals(suite: &MailFixture, original: &Value, replay_id: &str) -> FixtureResult {
    let conflict = suite
        .host()?
        .mcp_reply(
            "message_send",
            suite.message_input(Some(replay_id), "changed literal content"),
        )
        .await?;
    assert!(conflict.is_error);
    assert_eq!(at(&conflict.value, "/serviceKind")?, &json!("itemConflict"));
    assert_eq!(at(&conflict.value, "/effect")?, &json!("none"));
    let (code, cli) = suite
        .cli_output(&[
            "message",
            "send",
            "--human-user",
            "--to",
            &suite.target.to_string(),
            "--text",
            "changed literal content",
            "--item-id",
            replay_id,
        ])
        .await?;
    assert_eq!(code, 4);
    assert_eq!(
        cli.pointer("/error/serviceKind"),
        Some(&json!("itemConflict"))
    );
    assert_eq!(cli.pointer("/error/effect"), Some(&json!("none")));
    assert_eq!(cli.get("target"), Some(&Value::Null));
    assert_eq!(
        cli.pointer("/error/data/existing"),
        conflict.value.pointer("/data/existing")
    );
    let malformed = suite
        .host()?
        .mcp_reply(
            "message_send",
            suite.message_input(Some("not-a-uuid"), "never admitted"),
        )
        .await?;
    assert!(malformed.is_error);
    assert_eq!(
        at(&malformed.value, "/serviceKind")?,
        &json!("invalidShape")
    );
    assert_eq!(at(&malformed.value, "/effect")?, &json!("none"));
    let (code, malformed) = suite
        .cli_output(&[
            "message",
            "send",
            "--human-user",
            "--to",
            &suite.target.to_string(),
            "--text",
            "never admitted",
            "--item-id",
            "not-a-uuid",
        ])
        .await?;
    assert_eq!(code, 2);
    assert_eq!(malformed.pointer("/error/effect"), Some(&json!("none")));
    assert_eq!(at(original, "/pushId")?, &json!(replay_id));
    Ok(())
}

pub async fn terminal_readback(suite: &MailFixture, item: &str, original: &Value) -> FixtureResult {
    let binding = crate::messaging_stopped_owner::sender_binding(suite.stopped_directory()?)?;
    let connection = sqlx_turso::TursoConnectOptions::new()
        .filename(suite.stopped_directory()?.join("interaction.sqlite"))
        .create_if_missing(false)
        .read_only(true)
        .connect()
        .await?;
    run_scenario(
        connection,
        async |connection: &mut sqlx_turso::TursoConnection| {
            let answer: String = sqlx_turso::sqlx::query_scalar(
                "SELECT result FROM message_send_answers WHERE item_id=?",
            )
            .bind(item)
            .fetch_one(&mut *connection)
            .await?;
            assert_eq!(serde_json::from_str::<Value>(&answer)?, *original);
            let mut transaction = connection.begin().await?;
            let observations =
                fleet_mail_delivery::test_support::snapshot::<_, RouterDeliveryTypes, _>(
                    &mut durable_steps::turso_store::TursoScope::new(
                        &mut transaction,
                        &crate::native_sql::SqlHost,
                    ),
                    fleet_mail_delivery::DeliveryHostRole::Machine,
                    &binding,
                )
                .await?;
            transaction.commit().await?;
            let rows = observations
                .iter()
                .filter(|row| row.category == "inbox" && row.identity == item)
                .collect::<Vec<_>>();
            assert_eq!(rows.len(), 1, "one retained local custody identity");
            let custody_ids: Vec<String> = sqlx_turso::sqlx::query_scalar(
                "SELECT item_id FROM _fleet_sync_inbox_items ORDER BY item_id",
            )
            .fetch_all(&mut *connection)
            .await?;
            assert_eq!(
                custody_ids,
                vec![item.to_owned()],
                "migration/replay must not invent another custody item"
            );
            let answer_ids: Vec<String> = sqlx_turso::sqlx::query_scalar(
                "SELECT item_id FROM message_send_answers ORDER BY item_id",
            )
            .fetch_all(&mut *connection)
            .await?;
            assert_eq!(
                answer_ids,
                vec![item.to_owned()],
                "retained first-answer inventory"
            );
            let push_ids: Vec<String> = sqlx_turso::sqlx::query_scalar(
                "SELECT push_id FROM router_pushes ORDER BY push_id",
            )
            .fetch_all(&mut *connection)
            .await?;
            assert_eq!(
                push_ids,
                vec![item.to_owned()],
                "terminal migration must retain exactly the original push"
            );
            let outbox_ids: Vec<String> = sqlx_turso::sqlx::query_scalar(
                "SELECT item_id FROM _fleet_sync_outbox_items ORDER BY item_id",
            )
            .fetch_all(&mut *connection)
            .await?;
            assert!(
                outbox_ids.is_empty(),
                "local terminal mail must not invent outgoing custody"
            );
            let row = rows.first().ok_or("terminal inbox row absent")?;
            assert_eq!(row.identity, item);
            assert_eq!(row.metadata.as_deref(), Some("0"));
            assert_eq!(row.payload_bytes.as_deref(), Some("0"));
            // Keep the physical payload-erasure oracle as well as SDK accounting;
            // the public observation's content is the validated header, not payload.
            let payload: Option<String> = sqlx_turso::sqlx::query_scalar(
                "SELECT payload FROM _fleet_sync_inbox_items WHERE item_id=?",
            )
            .bind(item)
            .fetch_one(&mut *connection)
            .await?;
            assert_eq!(payload, None);
            let record: Value =
                serde_json::from_str(row.content.as_deref().ok_or("terminal SDK header absent")?)?;
            assert_terminal_header(&record)?;
            Ok(())
        },
        async |connection| {
            connection.close().await?;
            Ok(())
        },
        || Ok(()),
    )
    .await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong stored header; parsing errors propagate."
)]
fn assert_terminal_header(record: &Value) -> FixtureResult {
    assert_eq!(at(record, "/route")?, &json!("local"));
    assert_eq!(record.pointer("/progress/kind"), Some(&json!("final")));
    assert_eq!(
        record.pointer("/progress/value/outcome/kind"),
        Some(&json!("delivered"))
    );
    Ok(())
}

async fn discard_response(
    suite: &mut MailFixture,
    request: Value,
    text: &str,
) -> FixtureResult<String> {
    let url = reqwest::Url::parse(&suite.host()?.mcp_url)?;
    let port = url.port().ok_or("owned MCP port absent")?;
    assert_eq!(url.host_str(), Some("127.0.0.1"));
    let body = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"message_send","arguments":request}}).to_string();
    let mut transport = tokio::net::TcpStream::connect(("127.0.0.1", port)).await?;
    transport.write_all(format!("POST {} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nMCP-Protocol-Version: 2025-11-25\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", url.path(), body.len()).as_bytes()).await?;
    // A one-byte buffer prevents reading body bytes while observing only HTTP headers.
    let mut reader = BufReader::with_capacity(1, transport);
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    assert!(
        line.starts_with("HTTP/1.1 200"),
        "lost-response HTTP status: {line}"
    );
    loop {
        line.clear();
        if reader.read_line(&mut line).await? == 0 {
            return Err("MCP headers truncated".into());
        }
        if line == "\r\n" {
            break;
        }
    }
    let frame = suite
        .peer_mut()?
        .expect_text_with_timeout(text, Duration::from_secs(15))
        .await?;
    // Deliberately drop the actual response transport without decoding its body.
    drop(reader);
    Ok(frame)
}

pub async fn lost_answers(suite: &mut MailFixture) -> FixtureResult {
    let item_identity = super::mail_fixture::identity();
    let item = item_identity.as_str();
    const TEXT: &str = "LM03 supplied identity with lost caller answer";
    let request = suite.message_input(Some(item), TEXT);
    let frame = tokio::time::timeout(
        Duration::from_secs(20),
        discard_response(suite, request.clone(), TEXT),
    )
    .await??;
    assert!(frame.contains(item));
    let recovered = suite.host()?.mcp("message_send", request).await?;
    assert_eq!(at(&recovered, "/pushId")?, &json!(item));
    assert_eq!(
        recovered.pointer("/receipt/outcome/kind"),
        Some(&json!("peerMessageWritten"))
    );
    assert_eq!(suite.peer()?.count_after_barrier().await?, 1);
    suite.stop_host().await?;
    terminal_readback(suite, item, &recovered).await?;
    suite.reopen_host().await?;
    assert_eq!(
        suite
            .host()?
            .mcp("message_send", suite.message_input(Some(item), TEXT))
            .await?,
        recovered
    );
    assert_eq!(suite.peer()?.count_after_barrier().await?, 1);
    let omitted = suite.message_input(None, "LM03 omitted identity caller loss");
    let first_frame = tokio::time::timeout(
        Duration::from_secs(20),
        discard_response(suite, omitted.clone(), "LM03 omitted identity"),
    )
    .await??;
    let first_id = first_frame
        .split("/push/")
        .nth(1)
        .and_then(|tail| tail.get(..36))
        .ok_or("peer frame omitted generated push link")?;
    uuid::Uuid::parse_str(first_id)?;
    let second = suite.host()?.mcp("message_send", omitted).await?;
    assert_ne!(at(&second, "/pushId")?, &json!(first_id));
    suite
        .peer_mut()?
        .expect_marker("LM03 omitted identity")
        .await?;
    assert_eq!(suite.peer()?.count_after_barrier().await?, 3);
    assert_eq!(
        suite
            .host()?
            .mcp("message_send", suite.message_input(Some(item), TEXT))
            .await?,
        recovered
    );
    assert_eq!(suite.peer()?.count_after_barrier().await?, 3);
    suite.stop_host().await?;
    assert_eq!(suite.peer()?.count_after_barrier().await?, 3);
    Ok(())
}
