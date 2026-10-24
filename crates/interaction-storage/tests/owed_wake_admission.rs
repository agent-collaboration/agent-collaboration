//! Real owning-file replay and conflict checks for automation's owed wake input.
use agent_automation::{OccurrenceId, WakeupId};
use chrono::{TimeZone, Utc};
use collaboration_protocol::{
    DeliveryOutcome, PushDeliveryState, PushHeaderFacts, PushId, PushKind, PushOrigin,
    PushRecordDraft, RouterOriginRef, SessionRef,
};
use interaction_storage::{InteractionStore, StorageError};
use sqlx::Connection;
use sqlx_turso::{TursoConnectOptions, TursoConnection};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn wake_draft() -> Result<PushRecordDraft, Box<dyn std::error::Error>> {
    Ok(PushRecordDraft {
        push_id: PushId::try_from(uuid::Uuid::now_v7().to_string())?,
        kind: PushKind::Wake,
        origin: PushOrigin::Router(PushKind::Wake),
        origin_router_ref: Some(
            RouterOriginRef::Wake {
                wakeup_id: WakeupId::generate(),
                occurrence_id: OccurrenceId::generate(),
            }
            .canonical_string()?,
        ),
        target: serde_json::from_value::<SessionRef>(serde_json::json!({
            "endpoint": {"serviceId":"018f47d2-24d5-7a68-b9ec-6f759c39458f","endpointId":"codex-local"},
            "sessionId":"owed-wake-target"
        }))?,
        mode: None,
        guard: None,
        reply_to_push_id: None,
        header_facts: PushHeaderFacts::Wake,
        body: Some("The original wake text".to_owned()),
        activity: None,
        created_at: Utc
            .timestamp_millis_opt(1_000)
            .single()
            .ok_or("fixture time")?,
    })
}

#[tokio::test]
async fn cold_replay_returns_the_same_push_without_resetting_its_outcome() -> TestResult {
    // Arrange: the first admission committed, but automation never acknowledged it.
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("interaction.sqlite");
    let draft = wake_draft()?;
    let store = InteractionStore::open(&path).await?;
    store.admit_owed_wake_push(draft.clone()).await?;
    store.close().await?;
    let mut observer =
        TursoConnection::connect_with(&TursoConnectOptions::new().filename(&path)).await?;
    sqlx::query("UPDATE router_pushes SET delivery_state='outcome_unknown',last_outcome_json='{\"outcome\":{\"kind\":\"unknown\"},\"reachability\":null,\"client\":null}',settled_at='1970-01-01T00:00:02.000000000Z'")
        .execute(&mut observer).await?;
    observer.close().await?;

    // Act: the restarted handoff presents the unchanged input again.
    let store = InteractionStore::open(&path).await?;
    let before = store
        .get_push_record(&draft.push_id)
        .await?
        .ok_or("missing pre-replay push")?;
    if before.delivery_state != PushDeliveryState::OutcomeUnknown {
        return Err("fixture did not retain the settled native row".into());
    }
    let replayed = store.admit_owed_wake_push(draft.clone()).await?;

    // Assert: admission preserves the durable carrier outcome and original identity.
    if replayed.push_id != draft.push_id
        || replayed.created_at != draft.created_at
        || replayed.delivery_state != PushDeliveryState::OutcomeUnknown
        || !matches!(
            replayed.last_outcome.map(|receipt| receipt.outcome),
            Some(DeliveryOutcome::Unknown)
        )
        || replayed.settled_at != Utc.timestamp_millis_opt(2_000).single()
    {
        return Err("cold replay reset the committed carrier outcome or identity".into());
    }
    store.close().await?;
    let mut observer =
        TursoConnection::connect_with(&TursoConnectOptions::new().filename(&path)).await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM router_pushes")
        .fetch_one(&mut observer)
        .await?;
    if count != 1 {
        return Err(format!("replay created {count} wake rows instead of one").into());
    }
    observer.close().await?;
    Ok(())
}

#[tokio::test]
async fn changed_owed_content_is_refused_without_modifying_the_first_push() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("interaction.sqlite");
    let store = InteractionStore::open(&path).await?;
    let draft = wake_draft()?;
    let original = store.admit_owed_wake_push(draft.clone()).await?;
    let mut changed_body = draft.clone();
    changed_body.body = Some("Different wake text".to_owned());
    let mut changed_target = draft.clone();
    changed_target.target.session_id = "different-target".to_owned().try_into()?;
    let mut changed_origin = draft.clone();
    changed_origin.origin_router_ref = wake_draft()?.origin_router_ref;
    let mut changed_time = draft.clone();
    changed_time.created_at += chrono::Duration::milliseconds(1);
    for changed in [changed_body, changed_target, changed_origin, changed_time] {
        if !matches!(
            store.admit_owed_wake_push(changed).await,
            Err(StorageError::PushContentConflict)
        ) {
            return Err("changed wake content was not refused as a conflict".into());
        }
    }
    let stored = store
        .get_push_record(&draft.push_id)
        .await?
        .ok_or("missing original")?;
    if serde_json::to_value(stored)? != serde_json::to_value(original)? {
        return Err("conflicting replay changed the first committed wake".into());
    }
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn non_wake_input_never_enters_the_owed_wake_repository() -> TestResult {
    let directory = tempfile::tempdir()?;
    let store = InteractionStore::open(directory.path().join("interaction.sqlite")).await?;
    let mut draft = wake_draft()?;
    draft.kind = PushKind::DirectMessage;
    draft.origin = PushOrigin::OwnerUnverified;
    draft.origin_router_ref = None;
    draft.mode = Some(collaboration_protocol::MessageDelivery::Queue);
    draft.header_facts = PushHeaderFacts::DirectMessage {
        sender_display_name: None,
    };
    if !matches!(
        store.admit_owed_wake_push(draft.clone()).await,
        Err(StorageError::InvalidRecord)
    ) || store.get_push_record(&draft.push_id).await?.is_some()
    {
        return Err("non-wake input entered owed wake admission".into());
    }
    store.close().await?;
    Ok(())
}
