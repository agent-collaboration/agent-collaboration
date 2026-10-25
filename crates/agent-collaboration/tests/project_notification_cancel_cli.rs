//! The root project CLI reaches requested cancellation over real frozen rows and SDK custody.
#[path = "../../collaboration-service/tests/support/board_fixture.rs"]
#[allow(dead_code)]
mod board_fixture;
#[path = "../../collaboration-service/tests/support/board_read_setup.rs"]
mod board_read_setup;
#[path = "support/closed_mail_session.rs"]
mod closed_mail_session;
#[path = "support/local_mail_surfaces.rs"]
#[allow(dead_code)]
mod local_mail_surfaces;
use board_fixture::native_mail_owner;
use board_fixture::{BoardFixture, TestError};
use collaboration_protocol::MailItemId;
use local_mail_surfaces::Surfaces;
use message_board::*;
use serde_json::{Value, json};
use std::time::Duration;

async fn cli(surface: &Surfaces, path: &std::path::Path) -> Result<(i32, Value), TestError> {
    let output = tokio::time::timeout(
        Duration::from_secs(10),
        tokio::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
            .kill_on_drop(true)
            .args(["project", "notification-cancel", "--request-file"])
            .arg(path)
            .args(["--json", "--service-directory"])
            .arg(surface.root.path())
            .output(),
    )
    .await??;
    let value = serde_json::from_slice(&output.stdout).map_err(|error| {
        format!(
            "project cancel CLI JSON {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    })?;
    Ok((output.status.code().unwrap_or(-1), value))
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn project_notification_cancel_cli_preserves_pending_authority_and_requester_without_recording_a_project_request()
-> Result<(), TestError> {
    let board = BoardFixture::start().await?;
    let (project, _) = board.project().await?;
    let session_root = tempfile::tempdir()?;
    let session =
        closed_mail_session::closed_session(session_root.path(), board_fixture::fixture_peer()?)
            .await?;
    let surface =
        Surfaces::start_with_project_admission(Some(session), Some(board._admission.clone()))
            .await?;
    let service: ServiceId = "00000000-0000-4000-8000-000000000002"
        .to_owned()
        .try_into()?;
    let recipient = Identity::Session {
        session: SessionRef {
            endpoint: SessionEndpointRef {
                service_id: service.clone(),
                endpoint_id: "claude-local".to_owned().try_into()?,
            },
            session_id: "project-cli-recipient".to_owned().try_into()?,
        },
    };
    let post = board_read_setup::post_at_placement(&board, Placement::Project { project_id: project.clone() },
        json!({"text":"actual notification producer","addressees":[NotifyTarget::Actor(recipient)]})).await?;
    let store = board
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("native project")?;
    let mut read = store.snapshot().await?;
    let intent = read
        .records(RecordKind::NotifyIntent)
        .await?
        .into_iter()
        .find_map(|record| match record {
            Record::NotifyIntent(intent) if intent.cause == post.clone().into() => {
                Some(intent.intent_id)
            }
            _ => None,
        })
        .ok_or("produced intent")?;
    read.finish().await?;
    let sdk = fleet_sync_protocol::ItemId::new(uuid::Uuid::now_v7())?;
    board
        ._admission
        .claim_intent(
            &project,
            &intent,
            Some(collaboration_service::IntentHandoffInput {
                item: sdk,
                enrollment: collaboration_service::IntentRecipientEnrollment {
                    service,
                    peer: fleet_sync_protocol::PeerId::new(uuid::Uuid::now_v7())?,
                },
            }),
        )
        .await?;
    let mut write = store.begin_admission(board.record_origin()?).await?;
    let epoch = write.admitted_epoch()?;
    let original = write
        .mail_handoff(&sdk.as_uuid().to_string().try_into()?)
        .await?
        .ok_or("frozen handoff")?;
    write.rollback().await?;
    let message_board_storage::MailHandoffItem::Delivery(envelope) = original.item else {
        return Err("frozen control".into());
    };
    let envelope: fleet_sync_protocol::DeliveryEnvelope<
        collaboration_service::RouterDeliveryTypes,
    > = serde_json::from_slice(&serde_json::to_vec(&*envelope)?)?;
    let fleet_sync_protocol::DeliveryEnvelope::Item(item) = envelope else {
        return Err("frozen cancel".into());
    };
    let mail = surface.mail_runtime();
    mail.enqueue_project_item(item, epoch).await?;
    mail.publish_status(serde_json::from_value(
        json!({"item":sdk,"status":{"kind":"sent","value":{"seq":7}},"version":1,"cancel":null}),
    )?)
    .await?;
    drop(mail);
    let mut read = store.snapshot().await?;
    let before = (
        read.high_water(),
        read.record_counts().await?,
        read.mail_handoff_head().await?,
    );
    read.finish().await?;
    let path = surface.root.path().join("notification-cancel.json");
    let cancel = MailItemId::generate();
    let mut request = board.request(
        &project,
        json!({"kind":"notificationCancel","cancelId":cancel,"target":sdk}),
    );
    request.as_object_mut().ok_or("project request")?.insert("actor".to_owned(), json!({
        "kind":"session","session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"not-project-lead"},
    }));
    std::fs::write(&path, serde_json::to_vec(&request)?)?;
    let (code, denied) = cli(&surface, &path).await?;
    assert_eq!(code, 4, "{denied}");
    assert!(denied.to_string().contains("roleRequired"), "{denied}");
    assert!(
        surface
            .interactions
            .local_cancel_delivery(&cancel.as_str().to_owned().try_into()?)
            .await?
            .is_none()
    );
    request
        .as_object_mut()
        .ok_or("project request")?
        .insert("actor".to_owned(), board.actor.clone());
    std::fs::write(&path, serde_json::to_vec(&request)?)?;
    let (code, answer) = cli(&surface, &path).await?;
    assert_eq!(code, 0, "{answer}");
    assert_eq!(
        answer.pointer("/result/record/cancel"),
        Some(&json!(cancel))
    );
    assert_eq!(
        answer.pointer("/result/record/outcome/kind"),
        Some(&json!("pending"))
    );
    let (code, replay) = cli(&surface, &path).await?;
    assert_eq!(code, 0, "{replay}");
    assert_eq!(answer.get("result"), replay.get("result"));
    let mut read = store.snapshot().await?;
    assert_eq!(
        (
            read.high_water(),
            read.record_counts().await?,
            read.mail_handoff_head().await?
        ),
        before
    );
    read.finish().await?;
    let saved = surface
        .interactions
        .local_cancel_delivery(&cancel.as_str().to_owned().try_into()?)
        .await?
        .ok_or("requested attribution")?;
    assert_eq!(
        serde_json::to_value(saved.attribution)?,
        json!({"kind":"byRequester","by":board.actor})
    );
    drop(store);
    surface.stop().await?;
    board.stop().await
}
