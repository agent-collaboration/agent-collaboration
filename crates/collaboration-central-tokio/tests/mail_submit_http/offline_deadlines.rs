//! Quiet mail expires through the existing timer, including after owner reopen.
#![allow(
    clippy::panic_in_result_fn,
    reason = "Literal assertions qualify permanent conformance cases."
)]
use super::{
    grants::{connect, start},
    relay::{operation, relay_request},
    *,
};
use board_sync_protocol::BoardDeliveryTypes;
use collaboration_central_conformance::{ConformanceClient, enroll_admitted_peer};
use fleet_sync_protocol::{DeliveryEnvelope, DeliveryItem, ItemId, PeerId};
use reqwest_websocket::{CloseCode, WebSocket};
use runtime_primitives::SyncTime;
use serde_json::{Value, json};

const SENDER: &str = "550e8400-e29b-41d4-a716-446655441041";
const RECIPIENT: &str = "550e8400-e29b-41d4-a716-446655441042";
const CREDENTIAL: &str = "offline-expiry-sender";

async fn status(socket: &mut WebSocket, item: ItemId, expected: Value) -> ConformanceResult<Value> {
    tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            let answer = operation(
                socket,
                relay_request("statusFetch", json!({"after":0,"limit":10})),
            )
            .await?;
            if let Some(event) = answer
                .pointer("/value/value/events")
                .and_then(Value::as_array)
                .and_then(|events| {
                    events
                        .iter()
                        .rev()
                        .find(|event| event.get("item") == Some(&json!(item)))
                })
                && event.get("status") == Some(&expected)
            {
                return Ok(event.clone());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await?
}

async fn require_released_payload(base: &str) -> ConformanceResult<()> {
    let mut recipient = connect(
        base,
        "offline-expiry-recipient",
        &durable_steps::Uuid::now_v7().to_string(),
    )
    .await?;
    let fetched = operation(
        &mut recipient,
        relay_request(
            "fetch",
            json!({"stream":"items","after":0,"maxItems":10,"maxBytes":1_000_000}),
        ),
    )
    .await?;
    assert_eq!(
        fetched,
        json!({"kind":"delivery","value":{"kind":"fetch","value":{"stream":"items","entries":[],"through":1}}})
    );
    recipient.close(CloseCode::Normal, None).await?;
    Ok(())
}

#[tokio::test]
async fn close_publishes_newer_revocation_to_a_disconnected_sender_across_reopen()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let server = start(root.path(), &jwks).await?;
    let client = ConformanceClient::new(server.base_url())?;
    let mut sender = enroll_admitted_peer(
        &client,
        SENDER,
        "019f0000-0000-7000-8000-000000001041",
        CREDENTIAL,
        "019f0000-0000-7000-8000-000000011041",
    )
    .await?;
    let recipient = enroll_admitted_peer(
        &client,
        RECIPIENT,
        "019f0000-0000-7000-8000-000000001042",
        "close-publication-recipient",
        "019f0000-0000-7000-8000-000000011042",
    )
    .await?;
    let id = ItemId::new(durable_steps::Uuid::now_v7())?;
    let item = DeliveryEnvelope::Item(DeliveryItem::<BoardDeliveryTypes>::new(
        id,
        serde_json::from_value(json!({"kind":"humanUser","installation":SENDER}))?,
        serde_json::from_value(
            json!({"peer":PeerId::new(recipient.peer)?,"address":{"endpoint":{"serviceId":RECIPIENT,"endpointId":"codex-local"},"sessionId":"closed recipient"}}),
        )?,
        None,
        serde_json::from_value(
            json!({"kind":"message","text":"Close status survives","delivery":"auto","guard":null,"senderDisplayName":null,"replyTo":null}),
        )?,
    )?);
    operation(&mut sender.socket,json!({"kind":"consumer","value":{"kind":"mailSubmit","value":{"item":item,"originEpoch":null}}})).await?;
    let sent = status(
        &mut sender.socket,
        id,
        json!({"kind":"sent","value":{"seq":1}}),
    )
    .await?;
    sender.socket.close(CloseCode::Normal, None).await?;
    recipient.socket.close(CloseCode::Normal, None).await?;
    let revoked: Value = reqwest::Client::new()
        .post(format!("{}/admin/v1/machines/revoke", server.base_url()))
        .header(
            "Cf-Access-Jwt-Assertion",
            collaboration_central_conformance::owner_assertion()?,
        )
        .json(&json!({"kind":"revokeMachine","value":{"installation":RECIPIENT}}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(revoked, json!({"kind":"machineRevoked"}));
    server.shutdown().await?;
    let reopened = start(root.path(), &jwks).await?;
    let mut socket = connect(
        reopened.base_url(),
        CREDENTIAL,
        &durable_steps::Uuid::now_v7().to_string(),
    )
    .await?;
    let revoked = status(
        &mut socket,
        id,
        json!({"kind":"refused","value":{"reason":{"kind":"recipientRevoked"}}}),
    )
    .await?;
    assert_eq!(revoked.get("version"), Some(&json!(2)));
    assert!(
        revoked.get("cursor").and_then(Value::as_u64) > sent.get("cursor").and_then(Value::as_u64)
    );
    socket.close(CloseCode::Normal, None).await?;
    reopened.shutdown().await?;
    jwks.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn never_fetched_deadlines_expire_after_empty_lookup_and_reopen_while_fetched_and_no_ttl_remain()
-> ConformanceResult<()> {
    for (reopen, fetched, ttl) in [
        (false, false, true),
        (true, false, true),
        (false, true, true),
        (false, false, false),
    ] {
        let root = tempfile::tempdir()?;
        let jwks = JwksFixtureServer::start().await?;
        let server = start(root.path(), &jwks).await?;
        let client = ConformanceClient::new(server.base_url())?;
        let mut sender = enroll_admitted_peer(
            &client,
            SENDER,
            "019f0000-0000-7000-8000-000000001041",
            CREDENTIAL,
            "019f0000-0000-7000-8000-000000011041",
        )
        .await?;
        let mut recipient = enroll_admitted_peer(
            &client,
            RECIPIENT,
            "019f0000-0000-7000-8000-000000001042",
            "offline-expiry-recipient",
            "019f0000-0000-7000-8000-000000011042",
        )
        .await?;
        // Enrollment opened the empty mailbox before this later submission.
        let deadline = chrono::Utc::now().timestamp_millis() + 1500;
        let id = ItemId::new(durable_steps::Uuid::now_v7())?;
        let item = DeliveryEnvelope::Item(DeliveryItem::<BoardDeliveryTypes>::new(
            id,
            serde_json::from_value(json!({"kind":"humanUser","installation":SENDER}))?,
            serde_json::from_value(
                json!({"peer":PeerId::new(recipient.peer)?,"address":{"endpoint":{"serviceId":RECIPIENT,"endpointId":"codex-local"},"sessionId":"offline recipient"}}),
            )?,
            ttl.then_some(SyncTime::from_unix_millis(deadline)),
            serde_json::from_value(
                json!({"kind":"message","text":"quiet deadline","delivery":"auto","guard":null,"senderDisplayName":null,"replyTo":null}),
            )?,
        )?);
        let submitted=operation(&mut sender.socket,json!({"kind":"consumer","value":{"kind":"mailSubmit","value":{"item":item,"originEpoch":null}}})).await?;
        assert_eq!(
            submitted.pointer("/value/value/kind"),
            Some(&json!("submitted"))
        );
        let sent = status(
            &mut sender.socket,
            id,
            json!({"kind":"sent","value":{"seq":1}}),
        )
        .await?;
        assert_eq!(sent.get("version"), Some(&json!(1)));
        if fetched {
            let answer = operation(
                &mut recipient.socket,
                relay_request(
                    "fetch",
                    json!({"stream":"items","after":0,"maxItems":10,"maxBytes":1_000_000}),
                ),
            )
            .await?;
            assert_eq!(
                answer.pointer("/value/value/entries/0/value/item"),
                Some(&serde_json::to_value(&item)?)
            );
        }
        recipient.socket.close(CloseCode::Normal, None).await?;
        if reopen {
            sender.socket.close(CloseCode::Normal, None).await?;
            server.shutdown().await?;
            assert!(
                chrono::Utc::now().timestamp_millis() < deadline,
                "fixture missed reopen-before-due boundary"
            );
            let reopened = start(root.path(), &jwks).await?;
            let mut socket = connect(
                reopened.base_url(),
                CREDENTIAL,
                &durable_steps::Uuid::now_v7().to_string(),
            )
            .await?;
            let expired = status(&mut socket, id, json!({"kind":"expired"})).await?;
            assert_eq!(expired.get("version"), Some(&json!(2)));
            require_released_payload(reopened.base_url()).await?;
            socket.close(CloseCode::Normal, None).await?;
            reopened.shutdown().await?;
        } else {
            if ttl && !fetched {
                let expired = status(&mut sender.socket, id, json!({"kind":"expired"})).await?;
                assert_eq!(expired.get("version"), Some(&json!(2)));
                require_released_payload(server.base_url()).await?;
            } else {
                let wait =
                    u64::try_from((deadline + 200 - chrono::Utc::now().timestamp_millis()).max(0))?;
                tokio::time::sleep(Duration::from_millis(wait)).await;
                let retained = status(
                    &mut sender.socket,
                    id,
                    json!({"kind":"sent","value":{"seq":1}}),
                )
                .await?;
                assert_eq!(retained.get("version"), Some(&json!(1)));
            }
            sender.socket.close(CloseCode::Normal, None).await?;
            server.shutdown().await?;
        }
        jwks.shutdown().await?;
    }
    Ok(())
}
