//! Real admission writes bind intent state and frozen mail under one native guard.
#![expect(
    clippy::panic_in_result_fn,
    reason = "Literal behavioral assertions in Result-returning native fixtures."
)]
use super::subscription_fixture::*;
use crate::{IntentHandoffInput, IntentRecipientEnrollment, IntentReply, ProjectAdmissionError};
use fleet_sync_protocol::{DeliveryEnvelope, ItemId, PeerId};
use message_board::*;
use message_board_storage::{MailHandoffItem, ProjectStorageError};
use sqlx::{ConnectOptions, Connection};

async fn arrange_intent() -> Result<(SubscriptionFixture, NotifyIntent), TestError> {
    let fixture = SubscriptionFixture::start().await?;
    let post = PostId::generate();
    let request = ProjectRequest {
        request_id: RequestId::generate(),
        project_id: fixture.project.clone(),
        actor: human("handoff-writer")?,
        acting_for: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation: BoardOperation::ThreadPost {
            post_id: post.clone(),
            thread_id: fixture.root.clone(),
            text: PostText::try_from("frozen intent body".to_owned())?,
            references: Vec::new(),
            addressees: vec![NotifyTarget::Actor(fixture.reader.clone())],
        },
    };
    assert_accepted(&fixture.admission.submit_at(&request, fixture.now).await?);
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    let intent = read
        .records(RecordKind::NotifyIntent)
        .await?
        .into_iter()
        .find_map(|record| match record {
            Record::NotifyIntent(intent) if intent.cause == post.clone().into() => Some(intent),
            _ => None,
        })
        .ok_or("addressed intent")?;
    read.finish().await?;
    Ok((fixture, intent))
}

fn input(recipient: &Identity) -> Result<IntentHandoffInput, TestError> {
    let Identity::Session { session } = recipient else {
        return Err("session recipient".into());
    };
    Ok(IntentHandoffInput {
        item: ItemId::new(uuid::Uuid::now_v7())?,
        enrollment: IntentRecipientEnrollment {
            service: session.endpoint.service_id.clone(),
            peer: PeerId::new(uuid::Uuid::parse_str(
                "0192a1b2-c3d4-7000-8000-000000000001",
            )?)?,
        },
    })
}

#[tokio::test]
async fn intent_claim_commits_the_frozen_item_and_equal_reference_and_replays_without_reallocation()
-> Result<(), TestError> {
    let (fixture, original) = Box::pin(arrange_intent()).await?;
    let proposed = input(&fixture.reader)?;
    let expected_id = proposed.item.as_uuid().to_string();
    assert!(matches!(
        fixture
            .admission
            .claim_intent(&fixture.project, &original.intent_id, Some(proposed))
            .await?,
        IntentReply::Recorded {
            claim: Some(IntentClaim::Claimed { .. }),
            ..
        }
    ));
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    let Some(Record::NotifyIntent(claimed)) =
        read.record(&original.intent_id.clone().into()).await?
    else {
        return Err("claimed intent".into());
    };
    let IntentState::HandedOff { item } = claimed.state() else {
        return Err("handed off".into());
    };
    assert_eq!(item.as_str(), expected_id);
    let handoff = read.mail_handoff(item).await?.ok_or("handoff")?;
    assert_eq!(handoff.sequence.get(), 1);
    let frozen = serde_json::to_value(&handoff.item)?;
    let MailHandoffItem::Delivery(envelope) = handoff.item else {
        return Err("delivery".into());
    };
    assert_eq!(envelope.id().as_uuid().to_string(), expected_id);
    assert_eq!(
        serde_json::to_value(&envelope.recipient().address)?,
        serde_json::to_value(match &fixture.reader {
            Identity::Session { session } => session,
            Identity::Human { .. } => return Err("session".into()),
        })?
    );
    let DeliveryEnvelope::Item(item) = *envelope else {
        return Err("item".into());
    };
    let board_sync_protocol::BoardDeliveryBody::Notification(body) = item.body() else {
        return Err("notification".into());
    };
    assert_eq!(body.excerpt.as_str(), "frozen intent body");
    assert_eq!(body.pointer.subject, original.cause);
    let head = read.high_water();
    read.finish().await?;
    fixture
        .admission
        .claim_intent(
            &fixture.project,
            &original.intent_id,
            Some(input(&fixture.reader)?),
        )
        .await?;
    let mut read = store.snapshot().await?;
    let page = read.mail_handoff_page(HandoffSeq::ZERO, 10).await?;
    assert_eq!(page.len(), 1);
    assert_eq!(serde_json::to_value(&page[0].item)?, frozen);
    assert!(read.high_water().position >= head.position);
    read.finish().await?;
    Ok(())
}

#[tokio::test]
async fn a_missing_enrollment_defers_without_writes_and_a_resolved_pair_can_retry()
-> Result<(), TestError> {
    let (fixture, intent) = Box::pin(arrange_intent()).await?;
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    let head = read.high_water();
    let ledger = read.records(RecordKind::Request).await?;
    read.finish().await?;
    assert!(matches!(
        fixture
            .admission
            .claim_intent(&fixture.project, &intent.intent_id, None)
            .await?,
        IntentReply::Deferred { .. }
    ));
    let mut read = store.snapshot().await?;
    assert_eq!(read.high_water(), head);
    assert_eq!(read.records(RecordKind::Request).await?, ledger);
    assert_eq!(
        read.record(&intent.intent_id.clone().into()).await?,
        Some(Record::NotifyIntent(intent.clone()))
    );
    assert!(
        read.mail_handoff_page(HandoffSeq::ZERO, 10)
            .await?
            .is_empty()
    );
    read.finish().await?;
    assert!(matches!(
        fixture
            .admission
            .claim_intent(
                &fixture.project,
                &intent.intent_id,
                Some(input(&fixture.reader)?)
            )
            .await?,
        IntentReply::Recorded {
            claim: Some(IntentClaim::Claimed { .. }),
            ..
        }
    ));
    Ok(())
}

#[tokio::test]
async fn a_claim_refuses_an_enrollment_pair_for_a_different_service_under_the_lock()
-> Result<(), TestError> {
    let (fixture, intent) = Box::pin(arrange_intent()).await?;
    let mut proposed = input(&fixture.reader)?;
    proposed.enrollment.service = "00000000-0000-4000-8000-000000000099"
        .to_owned()
        .try_into()?;
    let result = fixture
        .admission
        .claim_intent(&fixture.project, &intent.intent_id, Some(proposed))
        .await;
    assert!(
        matches!(result, Err(ProjectAdmissionError::InvalidRecord(ref field)) if field == "intent recipient enrollment service")
    );
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    assert_eq!(
        read.record(&intent.intent_id.clone().into()).await?,
        Some(Record::NotifyIntent(intent))
    );
    assert!(
        read.mail_handoff_page(HandoffSeq::ZERO, 10)
            .await?
            .is_empty()
    );
    read.finish().await?;
    Ok(())
}

#[tokio::test]
async fn the_admission_writer_refuses_a_handed_off_reference_that_names_another_frozen_item()
-> Result<(), TestError> {
    let (fixture, mut intent) = Box::pin(arrange_intent()).await?;
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await?
        .ok_or("project")?;
    let mut write = store
        .begin_admission(crate::test_project_runtime::origin()?)
        .await?;
    let frozen = super::super::intent_handoff::freeze_intent_handoff(
        &mut write,
        &intent,
        &fixture.reader,
        &input(&fixture.reader)?,
    )
    .await?;
    intent.set_state(IntentState::HandedOff {
        item: MailItemRef::try_from(uuid::Uuid::now_v7().to_string())?,
    })?;
    let result =
        super::super::intent_handoff::record_intent_handoff(&mut write, &intent, &frozen).await;
    assert!(
        matches!(result, Err(ProjectAdmissionError::InvalidRecord(ref field)) if field == "intent handoff item reference")
    );
    write.rollback().await?;
    let mut read = store.snapshot().await?;
    assert!(
        read.mail_handoff_page(HandoffSeq::ZERO, 10)
            .await?
            .is_empty()
    );
    read.finish().await?;
    Ok(())
}

#[tokio::test]
async fn an_unknown_installation_is_a_terminal_refusal_without_a_frozen_handoff()
-> Result<(), TestError> {
    let (fixture, intent) = Box::pin(arrange_intent()).await?;
    let proposed = input(&fixture.reader)?;
    fixture
        .admission
        .refuse_unknown_intent_recipient(
            &fixture.project,
            &intent.intent_id,
            proposed.enrollment.service,
        )
        .await?;
    fixture
        .admission
        .claim_intent(
            &fixture.project,
            &intent.intent_id,
            Some(input(&fixture.reader)?),
        )
        .await?;
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    let Some(Record::NotifyIntent(current)) = read.record(&intent.intent_id.into()).await? else {
        return Err("intent".into());
    };
    assert!(matches!(
        current.state(),
        IntentState::Refused {
            reason: IntentRefusal::RecipientUnknown {}
        }
    ));
    assert!(
        read.mail_handoff_page(HandoffSeq::ZERO, 10)
            .await?
            .is_empty()
    );
    read.finish().await?;
    Ok(())
}

#[tokio::test]
async fn a_ledger_write_failure_rolls_back_the_frozen_handoff_and_intent_transition_together()
-> Result<(), TestError> {
    let (fixture, intent) = Box::pin(arrange_intent()).await?;
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    let head = read.high_water();
    let ledger = read.records(RecordKind::Request).await?;
    read.finish().await?;
    let mut fault = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .create_if_missing(false)
        .connect()
        .await?;
    sqlx::raw_sql("CREATE TRIGGER fail_handoff_ledger BEFORE INSERT ON request_decisions BEGIN SELECT RAISE(ABORT, 'fixture handoff ledger failure'); END").execute(&mut fault).await?;
    let result = fixture
        .admission
        .claim_intent(
            &fixture.project,
            &intent.intent_id,
            Some(input(&fixture.reader)?),
        )
        .await;
    assert!(
        matches!(result, Err(ProjectAdmissionError::Storage(ProjectStorageError::Database(ref error))) if error.to_string().contains("fixture handoff ledger failure")),
        "{:?}",
        result.as_ref().err()
    );
    sqlx::query("DROP TRIGGER fail_handoff_ledger")
        .execute(&mut fault)
        .await?;
    fault.close().await?;
    let mut read = store.snapshot().await?;
    assert_eq!(read.high_water(), head);
    assert_eq!(read.records(RecordKind::Request).await?, ledger);
    assert_eq!(
        read.record(&intent.intent_id.clone().into()).await?,
        Some(Record::NotifyIntent(intent))
    );
    assert!(
        read.mail_handoff_page(HandoffSeq::ZERO, 10)
            .await?
            .is_empty()
    );
    read.finish().await?;
    Ok(())
}
