//! Owning native transactions apply automation's owed schedule outcome exactly once.
use agent_automation::{RunId, ScheduleId};
use chrono::{TimeZone, Utc};
use collaboration_protocol::{
    DeliveryOutcome, DeliveryReceipt, PushDeliveryState, PushHeaderFacts, PushId, PushKind,
    PushOrigin, PushRecordDraft, RouterOriginRef,
};
use interaction_storage::{InteractionStore, StorageError};
use sqlx::Connection;
use sqlx_turso::{TursoConnectOptions, TursoConnection};
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn schedule_draft() -> TestResult<(PushRecordDraft, RouterOriginRef)> {
    let schedule_id = ScheduleId::generate();
    let run_id = RunId::generate();
    let origin = RouterOriginRef::ScheduleRun {
        schedule_id: schedule_id.clone(),
        run_id: run_id.clone(),
    };
    Ok((
        PushRecordDraft {
            push_id: PushId::try_from(uuid::Uuid::now_v7().to_string())?,
            kind: PushKind::ScheduleRun,
            origin: PushOrigin::Router(PushKind::ScheduleRun),
            origin_router_ref: Some(origin.canonical_string()?),
            target: serde_json::from_value(
                serde_json::json!({"endpoint":{"serviceId":"018f47d2-24d5-7a68-b9ec-6f759c39458f","endpointId":"codex-local"},"sessionId":"scheduled-target"}),
            )?,
            mode: None,
            guard: None,
            reply_to_push_id: None,
            header_facts: PushHeaderFacts::ScheduleRun {
                schedule_id,
                run_id,
            },
            body: Some("The captured scheduled run".to_owned()),
            activity: None,
            created_at: Utc
                .timestamp_millis_opt(1_000)
                .single()
                .ok_or("fixture time")?,
        },
        origin,
    ))
}
fn receipt() -> DeliveryReceipt {
    DeliveryReceipt {
        outcome: DeliveryOutcome::Queued,
        reachability: None,
        client: None,
    }
}

#[tokio::test]
async fn schedule_settlement_requires_attempted_origin_and_replays_without_a_write() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("interaction.sqlite");
    let (draft, origin) = schedule_draft()?;
    let id = draft.push_id.clone();
    let settled_at = Utc
        .timestamp_millis_opt(2_000)
        .single()
        .ok_or("fixture time")?;
    let owner = InteractionStore::open(&path).await?;
    let pending = owner.insert_push_record(draft).await?;
    if !matches!(
        owner
            .apply_schedule_run_push_outcome(
                &id,
                &origin,
                PushDeliveryState::Delivered,
                &receipt(),
                settled_at
            )
            .await,
        Err(StorageError::PushStateConflict)
    ) {
        return Err("pending schedule push accepted an owed settlement".into());
    }
    if serde_json::to_value(
        owner
            .get_push_record(&id)
            .await?
            .ok_or("missing pending push")?,
    )? != serde_json::to_value(pending)?
    {
        return Err("refused settlement changed the pending push".into());
    }
    owner.mark_schedule_push_attempted(&id).await?;
    let committed = owner
        .apply_schedule_run_push_outcome(
            &id,
            &origin,
            PushDeliveryState::Delivered,
            &receipt(),
            settled_at,
        )
        .await?;
    owner.close().await?;
    let owner = InteractionStore::open(&path).await?;
    let mut observer =
        TursoConnection::connect_with(&TursoConnectOptions::new().filename(&path)).await?;
    sqlx::raw_sql("CREATE TRIGGER reject_replayed_schedule_update BEFORE UPDATE ON router_pushes BEGIN SELECT RAISE(ABORT,'replay wrote the push again'); END").execute(&mut observer).await?;
    let replayed = owner
        .apply_schedule_run_push_outcome(
            &id,
            &origin,
            PushDeliveryState::Delivered,
            &receipt(),
            settled_at,
        )
        .await?;
    if serde_json::to_value(committed)? != serde_json::to_value(replayed)? {
        return Err("cold replay changed the stored result".into());
    }
    let (_, different_origin) = schedule_draft()?;
    if !matches!(
        owner
            .apply_schedule_run_push_outcome(
                &id,
                &different_origin,
                PushDeliveryState::Delivered,
                &receipt(),
                settled_at
            )
            .await,
        Err(StorageError::PushContentConflict)
    ) {
        return Err("different schedule origin was not refused".into());
    }
    let unknown = DeliveryReceipt {
        outcome: DeliveryOutcome::Unknown,
        reachability: None,
        client: None,
    };
    if !matches!(
        owner
            .apply_schedule_run_push_outcome(
                &id,
                &origin,
                PushDeliveryState::OutcomeUnknown,
                &unknown,
                settled_at
            )
            .await,
        Err(StorageError::PushStateConflict)
    ) {
        return Err("different settlement overwrote the terminal result".into());
    }
    sqlx::raw_sql("DROP TRIGGER reject_replayed_schedule_update")
        .execute(&mut observer)
        .await?;
    observer.close().await?;
    owner.close().await?;
    Ok(())
}

#[tokio::test]
async fn failed_schedule_settlement_rolls_back_and_releases_the_native_writer() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("interaction.sqlite");
    let (draft, origin) = schedule_draft()?;
    let id = draft.push_id.clone();
    let settled_at = Utc
        .timestamp_millis_opt(2_000)
        .single()
        .ok_or("fixture time")?;
    let owner = InteractionStore::open(&path).await?;
    owner.insert_push_record(draft).await?;
    let attempted = owner.mark_schedule_push_attempted(&id).await?;
    let mut observer =
        TursoConnection::connect_with(&TursoConnectOptions::new().filename(&path)).await?;
    sqlx::raw_sql("CREATE TRIGGER reject_schedule_settlement BEFORE UPDATE OF delivery_state ON router_pushes BEGIN SELECT RAISE(ABORT,'forced schedule settlement failure'); END").execute(&mut observer).await?;
    let refused = owner
        .apply_schedule_run_push_outcome(
            &id,
            &origin,
            PushDeliveryState::Delivered,
            &receipt(),
            settled_at,
        )
        .await;
    if !matches!(refused, Err(StorageError::Database(_))) {
        return Err("real settlement trigger did not fail the write".into());
    }
    // Another native connection can take the schema writer immediately on return.
    sqlx::raw_sql("DROP TRIGGER reject_schedule_settlement")
        .execute(&mut observer)
        .await?;
    if serde_json::to_value(owner.get_push_record(&id).await?.ok_or("attempt missing")?)?
        != serde_json::to_value(attempted)?
    {
        return Err("failed native settlement partially changed its attempted push".into());
    }
    observer.close().await?;
    owner
        .apply_schedule_run_push_outcome(
            &id,
            &origin,
            PushDeliveryState::Delivered,
            &receipt(),
            settled_at,
        )
        .await?;
    owner.close().await?;
    let reopened = InteractionStore::open(&path).await?;
    if reopened
        .get_push_record(&id)
        .await?
        .ok_or("cold push missing")?
        .delivery_state
        != PushDeliveryState::Delivered
    {
        return Err("restored native write did not survive reopen".into());
    }
    reopened.close().await?;
    Ok(())
}
