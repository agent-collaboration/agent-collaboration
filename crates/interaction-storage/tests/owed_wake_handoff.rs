//! Interrupted handoff keeps the real automation obligation and native push under one ID.
use agent_automation::{ExpiryRule, FirstFire, OperationId, TimingRule, WakeRecord};
use automation_storage::{AutomationStore, WakeCreate, WakeEvaluation};
use collaboration_protocol::{
    CodexGeneration, MessageContent, PushHeaderFacts, PushId, PushKind, PushOrigin,
    PushRecordDraft, RouterOriginRef, SavedMessage, SessionRef, WakeCreator,
};
use interaction_storage::{InteractionStore, StorageError};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use sqlx_turso::{TursoConnectOptions, TursoConnection};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn admission_draft(
    operation: &automation_storage::OwedPushOperation,
) -> TestResult<&PushRecordDraft> {
    match &operation.operation {
        automation_storage::PushOperation::Admit { push } => Ok(push.as_ref()),
        automation_storage::PushOperation::SettleScheduleRun { .. } => {
            Err("expected owed wake admission".into())
        }
    }
}

fn wake_draft(
    wake: &WakeRecord<SavedMessage, WakeCreator>,
    fire: &FirstFire,
) -> Result<PushRecordDraft, automation_storage::StorageError> {
    let invalid = || automation_storage::StorageError::InvalidRecord;
    let MessageContent::HumanUser { text } = &wake.definition.message.content else {
        return Err(invalid());
    };
    Ok(PushRecordDraft {
        push_id: PushId::try_from(uuid::Uuid::now_v7().to_string()).map_err(|_| invalid())?,
        kind: PushKind::Wake,
        origin: PushOrigin::Router(PushKind::Wake),
        origin_router_ref: Some(
            RouterOriginRef::Wake {
                wakeup_id: fire.wakeup_id.clone(),
                occurrence_id: fire.occurrence_id.clone(),
            }
            .canonical_string()
            .map_err(|_| invalid())?,
        ),
        target: wake.definition.message.target.clone(),
        mode: None,
        guard: None,
        reply_to_push_id: None,
        header_facts: PushHeaderFacts::Wake,
        body: Some(text.as_str().to_owned()),
        activity: None,
        created_at: chrono::DateTime::from_timestamp_millis(fire.fired_at_ms)
            .ok_or_else(invalid)?,
    })
}

async fn fire_wake(
    store: &mut AutomationStore,
) -> TestResult<(agent_automation::DeliveryId, PushRecordDraft)> {
    let saved: SavedMessage = serde_json::from_value(serde_json::json!({
        "target":{"endpoint":{"serviceId":"018f47d2-24d5-7a68-b9ec-6f759c39458f","endpointId":"codex-local"},"sessionId":"owed-native-target"},
        "content":{"kind":"humanUser","text":"The original fired wake"},"delivery":"queue",
        "generationGuard":{"serviceEpoch":"018f47d2-24d5-7a68-b9ec-6f759c39458f","generation":7}
    }))?;
    let wake = store
        .create_wakeup(&WakeCreate {
            requested_by: WakeCreator::Owner {},
            replace: false,
            operation_id: OperationId::generate(),
            message: saved,
            timing: TimingRule::After { seconds: 1 },
            expiry: ExpiryRule::None,
            now_ms: 0,
        })
        .await?;
    let mut original = None;
    let result = store
        .evaluate_wakeup::<SavedMessage>(&wake.definition.wakeup_id, 1_000, |wake, fire| {
            let draft = wake_draft(wake, fire)?;
            original = Some(draft.clone());
            Ok(draft)
        })
        .await?;
    let WakeEvaluation::Fired { delivery_id, .. } = result else {
        return Err("fixture wake did not fire".into());
    };
    Ok((
        delivery_id,
        original.ok_or("fired wake did not build its exact push input")?,
    ))
}

#[tokio::test]
async fn cold_handoff_after_fire_and_after_admission_preserves_one_push_and_claim() -> TestResult {
    // Arrange: automation committed the fire, with no interaction admission yet.
    let directory = tempfile::tempdir()?;
    let automation_path = directory.path().join("automation.sqlite");
    let interaction_path = directory.path().join("interaction.sqlite");
    let mut automation = AutomationStore::open(&automation_path).await?;
    let (delivery_id, original) = fire_wake(&mut automation).await?;
    automation.close().await?;

    // Act: reopen the owed input and commit it, losing the acknowledgement.
    let mut automation = AutomationStore::open(&automation_path).await?;
    let owed = automation.list_owed_push_operations(100).await?;
    if owed.len() != 1
        || admission_draft(&owed[0])? != &original
        || owed[0].owed_at.timestamp_millis() != 1_000
    {
        return Err("cold fire lost or changed the exact owed input".into());
    }
    let interaction = InteractionStore::open(&interaction_path).await?;
    let first = interaction
        .admit_owed_wake_push(admission_draft(&owed[0])?.clone())
        .await?;
    interaction.close().await?;
    automation.close().await?;

    // Act: another restart re-admits before deleting the owed row.
    let mut automation = AutomationStore::open(&automation_path).await?;
    let owed = automation.list_owed_push_operations(100).await?;
    if owed.len() != 1 {
        return Err("admission prematurely deleted the owed input".into());
    }
    let interaction = InteractionStore::open(&interaction_path).await?;
    let replayed = interaction
        .admit_owed_wake_push(admission_draft(&owed[0])?.clone())
        .await?;
    if serde_json::to_value(first)? != serde_json::to_value(replayed)? {
        return Err("lost acknowledgement changed the admitted push".into());
    }
    automation.acknowledge_owed_push_operation(&owed[0]).await?;
    automation.acknowledge_owed_push_operation(&owed[0]).await?;
    if !automation.list_owed_push_operations(100).await?.is_empty() {
        return Err("committed handoff retained its owed row".into());
    }
    let claim = automation
        .claim_delivery::<SessionRef, MessageContent, CodexGeneration>(&delivery_id, 1_000)
        .await?
        .ok_or("wake claim missing after handoff")?;
    if claim.mode != "queue"
        || claim
            .generation_guard
            .as_ref()
            .map(|guard| u64::from(guard.generation))
            != Some(7)
        || claim.target != original.target
    {
        return Err("handoff changed automation's delivery mode, guard or target".into());
    }
    automation.close().await?;
    interaction.close().await?;

    // Assert: independently observe the two committed files after all owners close.
    let mut native =
        TursoConnection::connect_with(&TursoConnectOptions::new().filename(&interaction_path))
            .await?;
    let pushes: i64 = sqlx::query_scalar("SELECT count(*) FROM router_pushes")
        .fetch_one(&mut native)
        .await?;
    native.close().await?;
    let mut old =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&automation_path))
            .await?;
    let legacy_push_tables: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name='router_pushes'",
    )
    .fetch_one(&mut old)
    .await?;
    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM owed_push_operations")
        .fetch_one(&mut old)
        .await?;
    old.close().await?;
    if (pushes, legacy_push_tables, remaining) != (1, 0, 0) {
        return Err(format!(
            "handoff durable counts were {pushes} native pushes/{legacy_push_tables} retired legacy tables/{remaining} owed operations, expected 1/0/0"
        )
        .into());
    }
    Ok(())
}

#[tokio::test]
async fn changed_input_cannot_replace_a_push_or_acknowledge_automation_debt() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut automation = AutomationStore::open(&directory.path().join("automation.sqlite")).await?;
    let (_, original) = fire_wake(&mut automation).await?;
    let interaction = InteractionStore::open(directory.path().join("interaction.sqlite")).await?;
    interaction.admit_owed_wake_push(original.clone()).await?;
    let mut changed_operation = automation
        .list_owed_push_operations(100)
        .await?
        .pop()
        .ok_or("missing owed operation")?;
    let automation_storage::PushOperation::Admit { push } = &mut changed_operation.operation else {
        return Err("wrong owed operation".into());
    };
    push.body = Some("Changed wake body".to_owned());
    let mut changed = original.clone();
    changed.body = Some("Changed wake body".to_owned());
    if !matches!(
        interaction.admit_owed_wake_push(changed.clone()).await,
        Err(StorageError::PushContentConflict)
    ) {
        return Err("changed native admission did not refuse its existing identity".into());
    }
    if !matches!(
        automation
            .acknowledge_owed_push_operation(&changed_operation)
            .await,
        Err(automation_storage::StorageError::PushContentConflict)
    ) {
        return Err("changed acknowledgement did not refuse its existing identity".into());
    }
    let retained = automation.list_owed_push_operations(100).await?;
    if retained.len() != 1 || admission_draft(&retained[0])? != &original {
        return Err("conflict deleted or changed the real owed input".into());
    }
    interaction.close().await?;
    automation.close().await?;
    Ok(())
}
