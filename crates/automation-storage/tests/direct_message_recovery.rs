use agent_automation::{OccurrenceId, RunId, ScheduleId, WakeupId};
use automation_storage::{AutomationStore, StorageError};
use chrono::Utc;
use collaboration_protocol::{
    CodexGeneration, DeliveryOutcome, DeliveryReceipt, EndpointId, EndpointRef, MessageDelivery,
    PushActivityRange, PushActivitySnapshot, PushDeliveryState, PushHeaderFacts, PushId, PushKind,
    PushOrigin, PushRecordDraft, RouterOriginRef, SessionId, SessionRef, UuidIdentity,
};
use message_board::{
    BatchId, EpochNumber, FeedCursor, FeedPosition, ProjectId, SubscriptionGeneration,
    SubscriptionScope, ThreadId,
};
use serde_json::{Value, json};
use sqlx::{Connection, sqlite::SqliteConnectOptions};
use std::path::PathBuf;

type TestError = Box<dyn std::error::Error + Send + Sync>;
type TestResult = Result<(), TestError>;

struct TestDatabase {
    path: PathBuf,
}

impl TestDatabase {
    fn new() -> Self {
        Self {
            path: std::env::temp_dir().join(format!(
                "router-direct-message-intent-{}.sqlite",
                uuid::Uuid::now_v7()
            )),
        }
    }
}

impl Drop for TestDatabase {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn ensure(condition: bool, message: &'static str) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn session(session_id: &str) -> Result<SessionRef, TestError> {
    Ok(SessionRef {
        endpoint: EndpointRef {
            service_id: UuidIdentity::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
            endpoint_id: EndpointId::try_from("codex-local".to_owned())?,
        },
        session_id: SessionId::try_from(session_id.to_owned())?,
    })
}

fn direct_message_draft(
    mode: MessageDelivery,
    guard: Option<CodexGeneration>,
) -> Result<PushRecordDraft, TestError> {
    let value = json!({
        "pushId":PushId::try_from(uuid::Uuid::now_v7().to_string())?,
        "kind":PushKind::DirectMessage,
        "origin":PushOrigin::Session(session("sender")?),
        "originRouterRef":null,
        "target":session("target")?,
        "mode":mode,
        "guard":guard,
        "replyToPushId":null,
        "headerFacts":PushHeaderFacts::DirectMessage { sender_display_name: None },
        "body":"persist this exact delivery intent",
        "activity":null,
        "createdAt":Utc::now(),
    });
    Ok(serde_json::from_value(value)?)
}

fn router_push_draft(
    target: &SessionRef,
    kind: PushKind,
    origin: RouterOriginRef,
    header_facts: PushHeaderFacts,
    body: Option<String>,
    activity: Option<PushActivitySnapshot>,
) -> Result<PushRecordDraft, TestError> {
    Ok(PushRecordDraft {
        push_id: PushId::try_from(uuid::Uuid::now_v7().to_string())?,
        kind,
        origin: PushOrigin::Router(kind),
        origin_router_ref: Some(origin.canonical_string()?),
        target: target.clone(),
        mode: None,
        guard: None,
        reply_to_push_id: None,
        header_facts,
        body,
        activity,
        created_at: Utc::now(),
    })
}

#[tokio::test]
async fn direct_message_delivery_intent_survives_store_and_reopen() -> TestResult {
    let database = TestDatabase::new();
    let mut store = AutomationStore::open(&database.path).await?;
    let guard: CodexGeneration = serde_json::from_value(json!({
        "serviceEpoch":"00000000-0000-4000-8000-000000000002","generation":3,
    }))?;
    let draft = direct_message_draft(MessageDelivery::Queue, Some(guard.clone()))?;
    let push_id = draft.push_id.clone();
    store.insert_push_record(draft).await?;
    store.close().await?;

    let mut reopened = AutomationStore::open(&database.path).await?;
    let record = reopened
        .get_push_record(&push_id)
        .await?
        .ok_or("stored DM missing after reopen")?;
    let encoded = serde_json::to_value(record)?;
    ensure(
        encoded["mode"] == json!("queue"),
        "queued intent was broadened",
    )?;
    ensure(
        encoded["guard"] == json!(guard),
        "generation guard was lost",
    )?;
    reopened.close().await?;
    Ok(())
}

#[tokio::test]
async fn corrupt_direct_message_mode_and_guard_fail_closed() -> TestResult {
    let database = TestDatabase::new();
    let mut store = AutomationStore::open(&database.path).await?;
    let draft = direct_message_draft(MessageDelivery::Auto, None)?;
    let push_id = draft.push_id.clone();
    store.insert_push_record(draft).await?;
    let mut observer =
        sqlx::SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&database.path))
            .await?;
    for (mode, guard) in [
        ("invented", None),
        (
            "auto",
            Some(json!({"serviceEpoch":"invalid","generation":3})),
        ),
    ] {
        let guard_json = guard.map(|guard: Value| guard.to_string());
        sqlx::query(
            "UPDATE router_pushes SET dm_delivery_mode=?,dm_generation_guard_json=? WHERE push_id=?",
        )
        .bind(mode)
        .bind(guard_json)
        .bind(push_id.as_str())
        .execute(&mut observer)
        .await?;
        ensure(
            matches!(
                store.get_push_record(&push_id).await,
                Err(StorageError::InvalidRecord)
            ),
            "corrupt DM intent was accepted or coerced",
        )?;
    }
    observer.close().await?;
    store.close().await?;
    Ok(())
}

#[tokio::test]
async fn restart_discovers_unsent_dm_targets_and_preserves_owner_recovery_attempts() -> TestResult {
    let database = TestDatabase::new();
    let mut store = AutomationStore::open(&database.path).await?;
    let first = direct_message_draft(MessageDelivery::Auto, None)?;
    let first_id = first.push_id.clone();
    store.insert_push_record(first).await?;
    let mut second = direct_message_draft(MessageDelivery::Queue, None)?;
    second.target = session("another-target")?;
    let second_id = second.push_id.clone();
    store.insert_push_record(second).await?;
    store
        .hold_unsent_direct_message(
            &second_id,
            DeliveryReceipt {
                outcome: DeliveryOutcome::NotSubmitted {
                    retryable: true,
                    reason: "known unsent".to_owned(),
                },
                reachability: None,
                client: None,
            },
        )
        .await?;
    let attempted = direct_message_draft(MessageDelivery::Steer, None)?;
    let attempted_id = attempted.push_id.clone();
    store.insert_push_record(attempted).await?;
    store.mark_push_attempted(&attempted_id).await?;
    let target = session("target")?;
    let root = ThreadId::generate();
    let activity = PushActivitySnapshot {
        ranges: vec![PushActivityRange {
            thread_id: root.clone(),
            project_id: ProjectId::generate(),
            from: FeedCursor {
                epoch: EpochNumber::try_from(1)?,
                position: FeedPosition::try_from(1)?,
            },
            through: FeedCursor {
                epoch: EpochNumber::try_from(1)?,
                position: FeedPosition::try_from(2)?,
            },
        }],
        held: false,
        draining: false,
    };
    let schedule_id = ScheduleId::generate();
    let run_id = RunId::generate();
    let router_attempts = vec![
        router_push_draft(
            &target,
            PushKind::Wake,
            RouterOriginRef::Wake {
                wakeup_id: WakeupId::generate(),
                occurrence_id: OccurrenceId::generate(),
            },
            PushHeaderFacts::Wake,
            Some("wake body".to_owned()),
            None,
        )?,
        router_push_draft(
            &target,
            PushKind::ScheduleRun,
            RouterOriginRef::ScheduleRun {
                schedule_id: schedule_id.clone(),
                run_id: run_id.clone(),
            },
            PushHeaderFacts::ScheduleRun {
                schedule_id,
                run_id,
            },
            Some("schedule body".to_owned()),
            None,
        )?,
        router_push_draft(
            &target,
            PushKind::Approval,
            RouterOriginRef::Interaction {
                interaction_id: collaboration_protocol::InteractionId::try_from(
                    "approval-interaction".to_owned(),
                )
                .expect("interaction id"),
                presentation_id: collaboration_protocol::InteractionPresentationId::generate(),
            },
            PushHeaderFacts::Approval {
                requester: session("approval-requester")?,
                requester_display_name: None,
            },
            Some("approval body".to_owned()),
            None,
        )?,
        router_push_draft(
            &target,
            PushKind::Question,
            RouterOriginRef::Interaction {
                interaction_id: collaboration_protocol::InteractionId::try_from(
                    "question-interaction".to_owned(),
                )
                .expect("interaction id"),
                presentation_id: collaboration_protocol::InteractionPresentationId::generate(),
            },
            PushHeaderFacts::Question {
                requester: session("question-requester")?,
                requester_display_name: None,
            },
            Some("question body".to_owned()),
            None,
        )?,
        router_push_draft(
            &target,
            PushKind::SubscriptionActivity,
            RouterOriginRef::SubscriptionActivity {
                target: target.clone(),
                batch_id: BatchId::generate(),
            },
            PushHeaderFacts::SubscriptionActivity {
                root_count: 1,
                message_count: 1,
                held_since: None,
                thread_resolved: false,
            },
            None,
            Some(activity),
        )?,
        router_push_draft(
            &target,
            PushKind::SubscriptionExpiry,
            RouterOriginRef::SubscriptionExpiry {
                target: target.clone(),
                scope: SubscriptionScope::thread(root),
                subscription_generation: SubscriptionGeneration::new(1).expect("generation"),
            },
            PushHeaderFacts::SubscriptionExpiry {
                scope: "thread expired-root".to_owned(),
            },
            Some("renew subscription".to_owned()),
            None,
        )?,
    ];
    let mut settleable_attempted_ids = vec![attempted_id.clone()];
    let mut owner_recovery_attempted_ids = Vec::new();
    for draft in router_attempts {
        let push_id = draft.push_id.clone();
        let kind = draft.kind;
        store.insert_push_record(draft).await?;
        store.mark_push_attempted(&push_id).await?;
        match kind {
            PushKind::Wake | PushKind::ScheduleRun => owner_recovery_attempted_ids.push(push_id),
            PushKind::Approval
            | PushKind::Question
            | PushKind::SubscriptionActivity
            | PushKind::SubscriptionExpiry
            | PushKind::BoardNotification => settleable_attempted_ids.push(push_id),
            PushKind::DirectMessage => return Err("unexpected DM fixture kind".into()),
        }
    }
    store.close().await?;

    let mut restored = AutomationStore::open(&database.path).await?;
    let attempted_count = u64::try_from(settleable_attempted_ids.len())?;
    ensure(
        restored.settle_interrupted_pushes(Utc::now()).await? == attempted_count,
        "recovery touched the wrong attempts",
    )?;
    let targets = restored.direct_message_recovery_targets().await?;
    ensure(
        targets.len() == 2
            && targets.contains(&session("target")?)
            && targets.contains(&session("another-target")?),
        "recovery missed cross-target DM work",
    )?;
    let pending = restored
        .get_push_record(&first_id)
        .await?
        .ok_or("pending missing")?;
    let held = restored
        .get_push_record(&second_id)
        .await?
        .ok_or("held missing")?;
    ensure(
        pending.delivery_state == PushDeliveryState::Pending,
        "known unsent pending was settled",
    )?;
    ensure(
        held.delivery_state == PushDeliveryState::Held && held.mode == Some(MessageDelivery::Queue),
        "unsubmitted hold did not survive crash",
    )?;
    for push_id in &settleable_attempted_ids {
        let unknown = restored
            .get_push_record(push_id)
            .await?
            .ok_or("attempted push missing")?;
        ensure(
            unknown.delivery_state == PushDeliveryState::OutcomeUnknown
                && unknown
                    .last_outcome
                    .is_some_and(|receipt| receipt.outcome == DeliveryOutcome::Unknown),
            "attempted push was replayable",
        )?;
    }
    for push_id in &owner_recovery_attempted_ids {
        let attempted = restored
            .get_push_record(push_id)
            .await?
            .ok_or("owner-recovery push missing")?;
        ensure(
            attempted.delivery_state == PushDeliveryState::Attempted
                && attempted.last_outcome.is_none(),
            "owner-recovery push was settled before its worker could recover it",
        )?;
    }
    ensure(
        restored.settle_interrupted_pushes(Utc::now()).await? == 0,
        "restore was not idempotent",
    )?;
    restored.close().await?;
    Ok(())
}

#[tokio::test]
async fn direct_message_intent_validation_rejects_missing_mode_and_non_dm_intent() -> TestResult {
    let mut missing = direct_message_draft(MessageDelivery::Auto, None)?;
    missing.mode = None;
    ensure(missing.into_pending().is_err(), "DM without mode accepted")?;
    let mut wake = direct_message_draft(MessageDelivery::Queue, None)?;
    wake.kind = PushKind::Wake;
    wake.origin = PushOrigin::Router(PushKind::Wake);
    wake.header_facts = PushHeaderFacts::Wake;
    ensure(wake.into_pending().is_err(), "non-DM mode accepted")?;
    Ok(())
}

#[tokio::test]
async fn board_notification_known_unsent_hold_survives_reopen_without_broadening_dms() -> TestResult
{
    let database = TestDatabase::new();
    let mut store = AutomationStore::open(&database.path).await?;
    let target = session("board-notification-target")?;
    let project = ProjectId::generate();
    let intent = message_board::NotifyIntentId::generate();
    let draft = router_push_draft(
        &target,
        PushKind::BoardNotification,
        RouterOriginRef::BoardIntent {
            project: project.clone(),
            intent: intent.clone(),
        },
        PushHeaderFacts::BoardNotification {
            project,
            intent,
            cause: message_board::PostId::generate().into(),
            summary: collaboration_protocol::MessageText::try_from(
                "known-unsent notification".to_owned(),
            )?,
        },
        None,
        None,
    )?;
    let push_id = draft.push_id.clone();
    let origin = draft.origin_router_ref.clone();
    store.insert_push_record(draft).await?;
    let receipt = DeliveryReceipt {
        outcome: DeliveryOutcome::NotSubmitted {
            retryable: true,
            reason: "wakeable".to_owned(),
        },
        reachability: None,
        client: None,
    };
    let held = store
        .hold_unsent_direct_message(&push_id, receipt.clone())
        .await?;
    ensure(
        held.delivery_state == PushDeliveryState::Held,
        "board notification did not hold",
    )?;
    ensure(
        held.origin_router_ref == origin,
        "hold changed board origin",
    )?;
    for (mode, guard) in [
        (MessageDelivery::Steer, None),
        (
            MessageDelivery::Auto,
            Some(serde_json::from_value(json!({
                "serviceEpoch":"00000000-0000-4000-8000-000000000002","generation":3
            }))?),
        ),
    ] {
        let dm = direct_message_draft(mode, guard)?;
        let id = dm.push_id.clone();
        store.insert_push_record(dm).await?;
        ensure(
            matches!(
                store.hold_unsent_direct_message(&id, receipt.clone()).await,
                Err(StorageError::InvalidRecord)
            ),
            "hold broadened DM restrictions",
        )?;
    }
    store.close().await?;
    let mut reopened = AutomationStore::open(&database.path).await?;
    let saved = reopened
        .get_push_record(&push_id)
        .await?
        .ok_or("held notification missing")?;
    ensure(
        saved.delivery_state == PushDeliveryState::Held,
        "reopen lost hold",
    )?;
    ensure(
        serde_json::to_value(&saved.last_outcome)? == serde_json::to_value(Some(receipt))?,
        "reopen lost known-unsent receipt",
    )?;
    ensure(saved.origin_router_ref == origin, "reopen changed origin")?;
    reopened.close().await?;
    Ok(())
}
