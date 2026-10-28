//! Real CLI/MCP remote custody; a closed session cannot supply a scripted queued answer.
use collaboration_service as mail_service;
#[path = "support/local_mail_surfaces.rs"]
// The local companion targets use the other fixture operations.
#[expect(
    dead_code,
    reason = "This test consumes a subset of the shared fixture and retains its fixed JSON construction."
)]
mod local_mail_surfaces;

#[path = "../../collaboration-service/tests/support/native_mail_owner.rs"]
#[expect(
    dead_code,
    reason = "This test consumes a subset of the shared fixture and retains its fixed JSON construction."
)]
mod native_mail_owner;
#[expect(
    dead_code,
    reason = "Shared native SQL fixture helpers serve different integration targets."
)]
#[path = "support/local_mail_native_sql.rs"]
// Companion targets use the application-row fault helpers.
mod native_sql;
use board_fleet_sync::{AccessServiceToken, MachineSessionStore, ReqwestSessionTransport};
use board_sync_protocol::BoardConsumerProtocol;
use collaboration_protocol::{MailItemId, SessionRef};
use collaboration_runtime::MailDeliverySession;
use durable_steps::AtomicStore;
use fleet_sync_protocol::{PeerId, SchemaSupport};
use fleet_sync_session::{Endpoint, PeerCredential, SessionClient};
use fleet_sync_tokio_runner::SessionPump;
use local_mail_surfaces::{SERVICE, Surfaces, TestResult};
use runtime_primitives::{Backoff, UuidV7Ids};
use serde_json::{Value, json};
use std::{sync::atomic::Ordering, time::Duration};

async fn offline_surfaces() -> TestResult<(tempfile::TempDir, Surfaces, SessionRef)> {
    let root = tempfile::tempdir()?;
    let peer = PeerId::new(uuid::Uuid::now_v7())?;
    let bound = Duration::from_secs(5);
    let client = SessionClient::<_, _, BoardConsumerProtocol>::load(
        ReqwestSessionTransport::new(AccessServiceToken::new("fixture", "fixture")?, bound, bound)?,
        MachineSessionStore::open(root.path()).await?,
        &UuidV7Ids,
        SchemaSupport { max_version: 1 },
        Backoff {
            initial: bound,
            max: bound,
            factor: 2,
        },
    )
    .await?;
    let (requests, events, pump) = SessionPump::new(
        client,
        Endpoint::new("ws://127.0.0.1:1/session")?,
        PeerCredential::new("Bearer offline-fixture")?,
        bound,
    );
    // No socket is opened. The configured session's real request channel is
    // closed, and its readiness remains false for this entire test lifetime.
    drop(events);
    drop(pump);
    let (_, welcomed) = tokio::sync::watch::channel(false);
    let surfaces = Surfaces::start_with_session(Some(MailDeliverySession {
        requests,
        welcomed,
        peer,
        relay: PeerId::new(uuid::Uuid::now_v7())?,
    }))
    .await?;
    let remote: SessionRef = serde_json::from_value(json!({
        "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000002","endpointId":"claude-local"},
        "sessionId":"remote-recipient",
    }))?;
    surfaces
        .interactions
        .remember_installation_peer(&interaction_storage::InstallationPeerBinding {
            service_id: String::from(remote.endpoint.service_id.clone()).try_into()?,
            peer_id: PeerId::new(uuid::Uuid::now_v7())?
                .as_uuid()
                .to_string()
                .try_into()?,
        })
        .await?;
    Ok((root, surfaces, remote))
}

async fn cli(surfaces: &Surfaces, arguments: &[&str]) -> TestResult<(i32, Value, String)> {
    let output = tokio::time::timeout(
        Duration::from_secs(10),
        tokio::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
            .kill_on_drop(true)
            .env("CODEX_THREAD_ID", "remote-cli-sender")
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .env_remove("CURSOR_CONVERSATION_ID")
            .args(arguments)
            .args(["--json", "--service-directory"])
            .arg(surfaces.root.path())
            .output(),
    )
    .await??;
    let stderr = String::from_utf8(output.stderr)?;
    let response = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("CLI response {error}; stderr={stderr}"))?;
    Ok((output.status.code().unwrap_or(-1), response, stderr))
}

fn sender() -> Value {
    json!({"endpoint":{"serviceId":SERVICE,"endpointId":"codex-local"},"sessionId":"remote-cli-sender"})
}
fn input(item: &MailItemId, target: &SessionRef, text: &str) -> Value {
    json!({"itemId":item,"target":target,"message":{"kind":"agent","sender":sender(),"text":text},"delivery":"auto"})
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn closed_session_remote_send_replays_across_cli_and_mcp_without_sender_push_or_local_injection()
-> TestResult {
    let (_session_root, surfaces, remote) = offline_surfaces().await?;
    let item = MailItemId::generate();
    let target = serde_json::to_string(&remote)?;
    let args = [
        "message",
        "send",
        "--to",
        target.as_str(),
        "--text",
        "remote custody",
        "--item-id",
        item.as_str(),
    ];
    let (code, first, printed) = cli(&surfaces, &args).await?;
    assert_eq!(code, 0, "{first}; {printed}");
    assert!(printed.contains(item.as_str()));
    assert_eq!(
        first.pointer("/result/record/queued/item"),
        Some(&json!(item))
    );
    assert_eq!(
        first.pointer("/result/record/queued/target"),
        Some(&json!(remote))
    );
    let replay = surfaces
        .api
        .call("message_send", input(&item, &remote, "remote custody"))
        .await?;
    assert_eq!(first.pointer("/result/record"), replay.get("result"));
    let (code, again, _) = cli(&surfaces, &args).await?;
    assert_eq!(code, 0);
    assert_eq!(
        again.pointer("/result/record"),
        first.pointer("/result/record")
    );
    let conflict = surfaces
        .api
        .call("message_send", input(&item, &remote, "different custody"))
        .await?;
    assert_eq!(
        conflict.pointer("/error/data/kind"),
        Some(&json!("itemConflict"))
    );
    assert!(
        surfaces
            .interactions
            .get_push_record(&item.clone().try_into()?)
            .await?
            .is_none()
    );
    assert!(
        surfaces
            .interactions
            .mail_submission_epoch(&item)
            .await?
            .is_none()
    );
    assert_eq!(surfaces.carrier.injections.load(Ordering::SeqCst), 0);
    let (code, shown, _) = cli(&surfaces, &["show", item.as_str()]).await?;
    assert_eq!(code, 0, "{shown}");
    assert_eq!(
        shown.pointer("/result/record/mail/status"),
        Some(&json!({"kind":"queued"}))
    );
    let (code, history, _) = cli(
        &surfaces,
        &[
            "message",
            "history",
            "--with",
            target.as_str(),
            "--limit",
            "1",
        ],
    )
    .await?;
    assert_eq!(code, 0, "{history}");
    assert_eq!(
        history.pointer("/result/page/records/0/mail/item"),
        Some(&json!(item)),
        "{history}"
    );
    // The fixture runs the real SDK engine, so admission may already have been
    // attempted. Supply a canonical Sent publication to establish this cancel
    // case deterministically; Pending must never promise a local withdrawal.
    let mail = surfaces.mail_runtime();
    mail.publish_status(serde_json::from_value(json!({
        "item":item,"status":{"kind":"sent","value":{"seq":7}},"version":1,"cancel":null,
    }))?)
    .await?;
    drop(mail);
    let cancel = MailItemId::generate();
    let (code, cancelled, printed) = cli(
        &surfaces,
        &[
            "message",
            "cancel",
            "--item",
            item.as_str(),
            "--cancel-id",
            cancel.as_str(),
        ],
    )
    .await?;
    assert_eq!(code, 0, "{cancelled}; {printed}");
    assert_eq!(
        cancelled.pointer("/result/record/outcome/kind"),
        Some(&json!("pending"))
    );
    let (code, shown, _) = cli(&surfaces, &["show", item.as_str()]).await?;
    assert_eq!(code, 0);
    assert_eq!(
        shown.pointer("/result/record/mail/status"),
        Some(&json!({"kind":"sent","seq":7}))
    );
    assert_eq!(surfaces.carrier.injections.load(Ordering::SeqCst), 0);
    surfaces.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn sender_show_reads_canonical_sdk_statuses_and_refusal_payloads_through_the_real_mcp()
-> TestResult {
    let (_session_root, surfaces, remote) = offline_surfaces().await?;
    let authority_item = MailItemId::generate();
    let queued = surfaces
        .api
        .call(
            "message_send",
            input(&authority_item, &remote, "read authority"),
        )
        .await?;
    assert_eq!(
        queued.pointer("/result/queued/item"),
        Some(&json!(authority_item))
    );
    for caller in [
        json!({"endpoint":{"serviceId":SERVICE,"endpointId":"codex-local"},"sessionId":"unrelated-session"}),
        json!({"kind":"humanUser"}),
    ] {
        let hidden = surfaces
            .api
            .call(
                "router_show",
                json!({"caller":caller,"reference":authority_item}),
            )
            .await?;
        assert_eq!(
            hidden.pointer("/error/data/kind"),
            Some(&json!("notPermitted")),
            "{hidden}"
        );
    }
    // Literal public expectations come from MS-3. Publications enter the real
    // SDK owner; only the external central's status input is supplied here.
    let mut cases = vec![
        (
            json!({"kind":"sent","value":{"seq":7}}),
            json!({"kind":"sent","seq":7}),
        ),
        (json!({"kind":"atRecipient"}), json!({"kind":"atMachine"})),
        (json!({"kind":"held"}), json!({"kind":"held"})),
        (
            json!({"kind":"delivered","value":{"acceptance":"peerMessageWritten"}}),
            json!({"kind":"delivered","acceptance":"peerMessageWritten"}),
        ),
        (json!({"kind":"uncertain"}), json!({"kind":"uncertain"})),
        (json!({"kind":"expired"}), json!({"kind":"expired"})),
        (json!({"kind":"cancelled"}), json!({"kind":"cancelled"})),
    ];
    for (native, public) in [
        (json!({"kind":"mailboxFull"}), json!({"kind":"mailboxFull"})),
        (
            json!({"kind":"recipientUnknown"}),
            json!({"kind":"recipientUnknown"}),
        ),
        (
            json!({"kind":"recipientRefused","value":{"code":"recipientUnsupported"}}),
            json!({"kind":"recipientRefused","code":"recipientUnsupported"}),
        ),
        (
            json!({"kind":"processingFailed","value":{"kind":"digestMismatch"}}),
            json!({"kind":"processingFailed","failure":"digestMismatch"}),
        ),
        (
            json!({"kind":"tooLarge","value":{"field":"address"}}),
            json!({"kind":"tooLarge","field":"address"}),
        ),
    ] {
        cases.push((
            json!({"kind":"refused","value":{"reason":native}}),
            json!({"kind":"refused","reason":public}),
        ));
    }
    for (native, expected) in cases {
        let item = MailItemId::generate();
        let queued = surfaces
            .api
            .call("message_send", input(&item, &remote, "canonical status"))
            .await?;
        assert_eq!(
            queued.pointer("/result/queued/item"),
            Some(&json!(item)),
            "{queued}"
        );
        let mail = surfaces.mail_runtime();
        mail.publish_status(serde_json::from_value(json!({
            "item":item,"status":native,"version":1,"cancel":null,
        }))?)
        .await?;
        drop(mail);
        let shown = surfaces
            .api
            .call("router_show", json!({"caller":sender(),"reference":item}))
            .await?;
        assert_eq!(
            shown.pointer("/result/mail/status"),
            Some(&expected),
            "{shown}"
        );
        assert!(
            surfaces
                .interactions
                .get_push_record(&item.try_into()?)
                .await?
                .is_none()
        );
    }
    assert_eq!(surfaces.carrier.injections.load(Ordering::SeqCst), 0);
    // Corrupt only the Router-owned companion. An invalid authority row cannot
    // become a guessed local push read or a status-by-ID escape hatch.
    native_sql::execute(
        surfaces.interactions.clone(),
        "UPDATE mail_read_contexts SET conversation_key='invalid'",
    )
    .await?;
    let corrupt = surfaces
        .api
        .call(
            "router_show",
            json!({"caller":sender(),"reference":authority_item}),
        )
        .await?;
    assert_eq!(
        corrupt.pointer("/error/data/kind"),
        Some(&json!("unavailable")),
        "{corrupt}"
    );
    surfaces.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn cli_reply_to_a_remote_origin_keeps_the_original_link_in_native_outbox_and_received_projection()
-> TestResult {
    let (_session_root, surfaces, remote) = offline_surfaces().await?;
    let original = collaboration_protocol::PushId::try_from(uuid::Uuid::now_v7().to_string())?;
    // The incoming message is an explicit prior-delivery fixture. The reply
    // producer, SDK transaction and receiving projection under test stay real.
    surfaces
        .interactions
        .insert_push_record(collaboration_protocol::PushRecordDraft {
            push_id: original.clone(),
            kind: collaboration_protocol::PushKind::DirectMessage,
            origin: collaboration_protocol::PushOrigin::Session(remote.clone()),
            origin_router_ref: None,
            target: serde_json::from_value(sender())?,
            reply_to_push_id: None,
            header_facts: collaboration_protocol::PushHeaderFacts::DirectMessage {
                sender_display_name: None,
            },
            body: Some("incoming remote body".to_owned()),
            activity: None,
            mode: Some(collaboration_protocol::MessageDelivery::Auto),
            guard: None,
            created_at: chrono::Utc::now(),
        })
        .await?;
    let (code, answer, printed) = cli(
        &surfaces,
        &["message", "reply", original.as_str(), "reply body"],
    )
    .await?;
    assert_eq!(code, 0, "{answer}; {printed}");
    let queued = answer
        .pointer("/result/record/queued")
        .ok_or_else(|| format!("{answer}"))?;
    assert_eq!(queued.get("target"), Some(&json!(remote)));
    let id: MailItemId = serde_json::from_value(queued.get("item").ok_or("reply item")?.clone())?;
    assert!(
        surfaces
            .interactions
            .get_push_record(&id.clone().try_into()?)
            .await?
            .is_none()
    );
    let sdk = fleet_sync_protocol::ItemId::new(uuid::Uuid::parse_str(id.as_str())?)?;
    let item = interaction_storage::InteractionAtomicStore::new(
        surfaces.interactions.clone(),
        std::sync::Arc::new(native_sql::SqlHost),
    )
    .transaction(move |scope| {
        Box::pin(async move {
            let limits = fleet_sync_delivery::DeliveryLimits::new::<
                collaboration_contracts::RouterDeliveryTypes,
            >(
                64 * 1024 * 1024,
                10_000,
                64 * 1024 * 1024,
                10_000,
                64 * 1024 * 1024,
                fleet_sync_delivery::minimum_settlement_allowance::<
                    collaboration_contracts::RouterDeliveryTypes,
                >()
                .map_err(durable_steps::StoreError::backend)?,
            )
            .map_err(durable_steps::StoreError::backend)?;
            // The SDK's supported read-only inspection exposes the persisted
            // header reference. Resolve the body through its public Outbox API;
            // no query reaches into the SDK's private tables.
            let rows = fleet_sync_delivery::test_support::snapshot(scope)
                .await
                .map_err(fleet_sync_delivery::delivery_rollback)?;
            let content = rows
                .iter()
                .find(|row| row.category == "outbox" && row.identity == sdk.as_uuid().to_string())
                .and_then(|row| row.content.as_ref())
                .ok_or_else(|| durable_steps::StoreError::backend("reply SDK header missing"))?;
            let header: Value =
                serde_json::from_str(content).map_err(durable_steps::StoreError::backend)?;
            let digest = header
                .pointer("/value/digest")
                .and_then(Value::as_str)
                .ok_or_else(|| durable_steps::StoreError::backend("reply SDK digest missing"))?
                .to_owned()
                .try_into()
                .map_err(durable_steps::StoreError::backend)?;
            let mut outbox = fleet_sync_delivery::Outbox::<
                _,
                collaboration_contracts::RouterDeliveryTypes,
            >::new(scope, limits);
            outbox
                .resolve_submit(&fleet_sync_protocol::DeliveryItemRef { item: sdk, digest })
                .await
                .map(|resolved| resolved.item)
                .map_err(fleet_sync_delivery::delivery_rollback)
        })
    })
    .await?;
    let fleet_sync_protocol::DeliveryEnvelope::Item(item) = item else {
        return Err("reply control".into());
    };
    let board_sync_protocol::BoardDeliveryBody::Message(body) = item.body() else {
        return Err("reply notification".into());
    };
    assert_eq!(
        body.reply_to.as_ref().map(|id| id.as_str()),
        Some(original.as_str())
    );
    assert_eq!(body.text.as_str(), "reply body");
    let projected = collaboration_runtime::received_mail_push(&item, chrono::Utc::now())?
        .ok_or("received reply")?;
    assert_eq!(projected.reply_to_push_id.as_ref(), Some(&original));
    assert_eq!(projected.body.as_deref(), Some("reply body"));
    assert_eq!(surfaces.carrier.injections.load(Ordering::SeqCst), 0);
    surfaces.stop().await
}
