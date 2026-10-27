//! An unacknowledged remote prefix cannot delay a later local notification.
use super::notification_owner::ProjectNotificationFixture;
use super::*;
use collaboration_service::{IntentHandoffInput, IntentRecipientEnrollment};
use fleet_sync_protocol::{ItemId, RecipientOutcome};
use message_board::*;

#[tokio::test]
async fn a_pending_remote_handoff_does_not_block_a_later_local_handoff() -> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    let mut fixture = ProjectNotificationFixture::open(directory.path()).await?;
    let remote = fixture.seed_item;
    // Freeze the later local handoff before Welcome starts the live publisher.
    // A manual claim after start would race its first-claim-only item selection.
    let local_service: ServiceId = "00000000-0000-4000-8000-000000000001"
        .to_owned()
        .try_into()?;
    let post = fixture
        .post("prefix-local-writer", Some(local_service.clone()))
        .await?;
    let intent = fixture.intent_for(&post).await?;
    let local = ItemId::new(durable_steps::Uuid::now_v7())?;
    let peer = fixture.harness.peer();
    fixture
        .admission
        .claim_intent(
            &fixture.project,
            &intent.intent_id,
            Some(IntentHandoffInput {
                item: local,
                enrollment: IntentRecipientEnrollment {
                    service: local_service,
                    peer,
                },
            }),
        )
        .await?;
    let local_intent = fixture.intent_for(&post).await?;
    let IntentState::HandedOff { item: frozen } = local_intent.state() else {
        return Err("local handoff missing".into());
    };
    assert_eq!(frozen.as_str(), local.as_uuid().to_string());
    let version = local_intent
        .envelope
        .position
        .project_cursor()
        .ok_or("local state cursor")?;
    let push: collaboration_protocol::PushId = local.as_uuid().to_string().try_into()?;
    let (entered, release) = fixture
        .harness
        .central
        .notifications
        .hold_next_piece()
        .await?;
    fixture.start().await?;
    tokio::time::timeout(BOUND, entered).await??;
    tokio::time::timeout(BOUND, async {
        loop {
            if fixture
                .harness
                .mail
                .owner
                .intents_scanned_through(&fixture.project)
                .await?
                .is_some()
            {
                return Ok::<_, interaction_storage::StorageError>(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;
    let prefix_before = fixture
        .harness
        .mail
        .owner
        .intents_scanned_through(&fixture.project)
        .await?;
    fixture
        .publisher
        .queue_project(fixture.project.clone())
        .await?;
    let local_before_ack = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let record = fixture.harness.mail.owner.get_push_record(&push).await?;
            let outcome = fixture.harness.mail.local_outcome(peer, local).await?;
            if let (Some(record), Some(RecipientOutcome::Held)) = (record, outcome)
                && fixture
                    .harness
                    .mail
                    .owner
                    .intent_consumed(&fixture.project, &local_intent.intent_id, version)
                    .await?
            {
                let _ = record;
                return fixture.show_local_push(&push).await;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    let remote_before_ack = fixture
        .harness
        .central
        .notifications
        .has_submission(remote)
        .await;
    let prefix_while_held = fixture
        .harness
        .mail
        .owner
        .intents_scanned_through(&fixture.project)
        .await?;
    let local_consumed_while_held = fixture
        .harness
        .mail
        .owner
        .intent_consumed(&fixture.project, &local_intent.intent_id, version)
        .await?;
    release
        .send(())
        .map_err(|_| io::Error::other("held copy closed"))?;
    let remote_after_ack = tokio::time::timeout(Duration::from_secs(10), async {
        fixture
            .harness
            .projects
            .replicate_project_for_test(&fixture.project)
            .await?;
        Ok::<_, TestError>(
            fixture
                .harness
                .central
                .notifications
                .wait_submission(remote)
                .await,
        )
    })
    .await
    .map_err(|_| io::Error::other("R was not published after acknowledgement"));
    // A later prompt must skip L's consumed version even though R held the
    // contiguous cursor below it during the earlier pass.
    fixture
        .publisher
        .queue_project(fixture.project.clone())
        .await?;
    tokio::time::timeout(BOUND, async {
        loop {
            if fixture
                .harness
                .mail
                .owner
                .intent_consumed(&fixture.project, &local_intent.intent_id, version)
                .await?
            {
                return Ok::<_, interaction_storage::StorageError>(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;
    let rows = fixture.harness.mail.rows().await?;
    let local_rows = rows
        .iter()
        .filter(|row| row.category == "inbox" && row.identity == local.as_uuid().to_string())
        .count();
    let remote_rows = rows
        .iter()
        .filter(|row| row.category == "outbox" && row.identity == remote.as_uuid().to_string())
        .count();
    let final_push = fixture.show_local_push(&push).await?;
    let final_outcome = fixture.harness.mail.local_outcome(peer, local).await?;
    let fixture_project = fixture.project.clone();
    let fixture_epoch = fixture.seed_epoch;
    fixture.close().await?;
    assert!(
        local_before_ack.is_ok(),
        "local mail waited behind the unacknowledged remote handoff"
    );
    let early_shown = local_before_ack??;
    let early_push = early_shown.record;
    assert!(
        early_push.body.is_none(),
        "intent notifications keep their content in the project"
    );
    assert!(matches!(&early_push.header_facts,
        collaboration_protocol::PushHeaderFacts::BoardNotification { project, intent, cause, .. }
        if project == &fixture_project && intent == &local_intent.intent_id && cause == &post.clone().into()
    ));
    assert_eq!(
        early_push.delivery_state,
        collaboration_protocol::PushDeliveryState::Held
    );
    assert!(
        !remote_before_ack,
        "remote mail bypassed its acknowledged prefix"
    );
    assert_eq!(
        prefix_while_held, prefix_before,
        "pending R did not hold the contiguous cursor"
    );
    assert!(
        local_consumed_while_held,
        "L was not recorded independently of the prefix"
    );
    assert_eq!(
        remote_after_ack??,
        Some(fleet_sync_protocol::Epoch::new(fixture_epoch.get())?)
    );
    assert_eq!(local_rows, 1, "L allocated duplicate native inbox custody");
    assert_eq!(
        remote_rows, 1,
        "R allocated duplicate native outgoing custody"
    );
    assert_eq!(final_push.link, early_shown.link);
    assert_eq!(final_push.record.push_id, early_push.push_id);
    assert_eq!(final_push.record.body, early_push.body);
    assert_eq!(final_push.record.delivery_state, early_push.delivery_state);
    assert!(matches!(final_outcome, Some(RecipientOutcome::Held)));
    Ok(())
}
