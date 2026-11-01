use super::*;
use fleet_admission::{FleetOperation, canonical_hash};
use message_board::*;
use sqlx::Connection;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

fn inputs() -> Result<(ProjectCreateRequest, FleetRequest), Box<dyn std::error::Error + Send + Sync>>
{
    let input = ProjectCreateRequest {
        request_id: RequestId::generate(),
        protocol_version: ProtocolVersion::CURRENT,
        project_id: ProjectId::generate(),
        board_id: BoardId::generate(),
        title: ResourceName::try_from("queued creation".to_owned())?,
        description: Some(Description::try_from(" original\ndescription ".to_owned())?),
        actor: Identity::Human {
            human_id: HumanId::try_from("owner".to_owned())?,
        },
        acting_for: None,
    };
    let directory = FleetRequest {
        request_id: input.request_id.clone(),
        workspace: WorkspaceId::generate(),
        actor: RecordedActor::declared(input.actor.clone()),
        acting_for: input.acting_for.clone(),
        protocol_version: input.protocol_version,
        operation: FleetOperation::ProjectDirectoryCreate {
            project: input.project_id.clone(),
            board: input.board_id.clone(),
            title: input.title.clone(),
        },
    };
    Ok((input, directory))
}

fn accepted_decision() -> Result<RequestDecision, Box<dyn std::error::Error + Send + Sync>> {
    Ok(RequestDecision::Accepted {
        effects: DecisionEffectsPage::from_full(
            Vec::new(),
            &DecisionEffectsHeader::from_full(&[]),
            None,
        )?,
        change: ChangeOutcome::Changed,
    })
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn complete_creation_input_and_identity_survive_pending_conflict_and_reopen() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let path = temporary.path().join("outbox.sqlite");
    let outbox = FleetOutbox::open(&path).await?;
    let (input, directory) = inputs()?;
    let queued = outbox.enqueue_creation(&directory, &input).await?;
    let mut conflicting = input.clone();
    conflicting.description = Some(Description::try_from("changed".to_owned())?);
    assert!(matches!(
        outbox.enqueue_creation(&directory, &conflicting).await,
        Err(RouterLocalStorageError::ConflictingRequest)
    ));
    assert_eq!(
        outbox.creation_input(&input.request_id).await?,
        Some(input.clone())
    );
    assert!(outbox.accepted_creation_page("", 100).await?.is_empty());
    outbox.close().await;
    let outbox = FleetOutbox::open(&path).await?;
    assert_eq!(outbox.request(&input.request_id).await?, Some(queued));
    assert_eq!(
        outbox.creation_input(&input.request_id).await?,
        Some(input.clone())
    );
    let accepted = accepted_decision()?;
    outbox
        .mark_decided(&input.request_id, &accepted, 1_000)
        .await?;
    assert_eq!(outbox.accepted_creation_page("", 100).await?, vec![input]);
    outbox.close().await;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn creation_input_failure_rolls_back_its_directory_request() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let outbox = FleetOutbox::open(&temporary.path().join("outbox.sqlite")).await?;
    let (input, directory) = inputs()?;
    // A fault at the second table must not leave the first half enqueued.
    sqlx::query("DROP TABLE project_creation_inputs")
        .execute(&outbox.database.pool)
        .await?;
    assert!(outbox.enqueue_creation(&directory, &input).await.is_err());
    assert!(outbox.request(&input.request_id).await?.is_none());
    outbox.close().await;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn authoritative_creation_decode_rejects_changed_input_and_pair_identity() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let outbox = FleetOutbox::open(&temporary.path().join("outbox.sqlite")).await?;
    let (input, directory) = inputs()?;
    outbox.enqueue_creation(&directory, &input).await?;
    let mut changed = input.clone();
    changed.description = Some(Description::try_from("corrupt description".to_owned())?);
    sqlx::query("UPDATE project_creation_inputs SET input = ? WHERE request_id = ?")
        .bind(serde_json::to_string(&changed)?)
        .bind(input.request_id.as_str())
        .execute(&outbox.database.pool)
        .await?;
    assert!(matches!(
        outbox.creation_input(&input.request_id).await,
        Err(RouterLocalStorageError::InvalidRecord)
    ));
    changed = input.clone();
    changed.project_id = ProjectId::generate();
    let hash = request_canonical::creation_hash(&changed)?;
    sqlx::query(
        "UPDATE project_creation_inputs SET input = ?, canonical_hash = ? WHERE request_id = ?",
    )
    .bind(serde_json::to_string(&changed)?)
    .bind(hash.as_str())
    .bind(input.request_id.as_str())
    .execute(&outbox.database.pool)
    .await?;
    assert!(matches!(
        outbox.creation_input(&input.request_id).await,
        Err(RouterLocalStorageError::InvalidRecord)
    ));
    outbox.close().await;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn decided_request_and_creation_input_prune_at_two_calendar_months_but_pending_survives()
-> TestResult {
    let temporary = tempfile::tempdir()?;
    let outbox = FleetOutbox::open(&temporary.path().join("outbox.sqlite")).await?;
    let (finished_input, finished_request) = inputs()?;
    outbox
        .enqueue_creation(&finished_request, &finished_input)
        .await?;
    let (pending_input, pending_request) = inputs()?;
    outbox
        .enqueue_creation(&pending_request, &pending_input)
        .await?;
    let decided_at =
        chrono::DateTime::parse_from_rfc3339("2026-10-31T12:30:00Z")?.timestamp_millis();
    outbox
        .mark_decided(
            &finished_request.request_id,
            &accepted_decision()?,
            decided_at,
        )
        .await?;
    let exact_boundary = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(decided_at)
        .ok_or("decision clock out of range")?
        .checked_add_months(chrono::Months::new(FLEET_OUTBOX_RETENTION_MONTHS))
        .ok_or("boundary out of range")?
        .timestamp_millis();

    assert_eq!(outbox.prune_expired(exact_boundary, 500).await?, 0);
    assert!(
        outbox
            .request(&finished_request.request_id)
            .await?
            .is_some()
    );
    assert!(
        outbox
            .creation_input(&finished_request.request_id)
            .await?
            .is_some()
    );
    assert_eq!(outbox.prune_expired(exact_boundary + 1, 500).await?, 1);
    assert!(
        outbox
            .request(&finished_request.request_id)
            .await?
            .is_none()
    );
    assert!(
        outbox
            .creation_input(&finished_request.request_id)
            .await?
            .is_none()
    );
    assert!(outbox.request(&pending_request.request_id).await?.is_some());
    assert_eq!(
        outbox.creation_input(&pending_request.request_id).await?,
        Some(pending_input)
    );
    outbox.close().await;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn migrated_decided_rows_age_from_migration_and_pending_rows_stay_unpruned() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let path = temporary.path().join("outbox.sqlite");
    let mut legacy = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true),
    )
    .await?;
    let earlier_migrations = OUTBOX_MIGRATOR.iter().take(4).cloned().collect();
    sqlx::migrate::Migrator::with_migrations(earlier_migrations)
        .run(&mut legacy)
        .await?;
    let (finished_input, finished_request) = inputs()?;
    let (pending_input, pending_request) = inputs()?;
    let finished_hash = canonical_hash(&finished_request)?;
    sqlx::query("INSERT INTO fleet_outbox_requests(request_id,canonical_hash,request,state) VALUES (?,?,?,?)")
        .bind(finished_request.request_id.as_str())
        .bind(finished_hash.as_str())
        .bind(serde_json::to_string(&finished_request)?)
        .bind(serde_json::to_string(&FleetOutboxState::Decided {
            decision: accepted_decision()?,
        })?)
        .execute(&mut legacy)
        .await?;
    let creation_hash = request_canonical::creation_hash(&finished_input)?;
    sqlx::query(
        "INSERT INTO project_creation_inputs(request_id,canonical_hash,input) VALUES (?,?,?)",
    )
    .bind(finished_input.request_id.as_str())
    .bind(creation_hash.as_str())
    .bind(serde_json::to_string(&finished_input)?)
    .execute(&mut legacy)
    .await?;
    let pending_hash = canonical_hash(&pending_request)?;
    sqlx::query("INSERT INTO fleet_outbox_requests(request_id,canonical_hash,request,state) VALUES (?,?,?,?)")
        .bind(pending_request.request_id.as_str())
        .bind(pending_hash.as_str())
        .bind(serde_json::to_string(&pending_request)?)
        .bind(serde_json::to_string(&FleetOutboxState::Pending {})?)
        .execute(&mut legacy)
        .await?;
    let pending_creation_hash = request_canonical::creation_hash(&pending_input)?;
    sqlx::query(
        "INSERT INTO project_creation_inputs(request_id,canonical_hash,input) VALUES (?,?,?)",
    )
    .bind(pending_input.request_id.as_str())
    .bind(pending_creation_hash.as_str())
    .bind(serde_json::to_string(&pending_input)?)
    .execute(&mut legacy)
    .await?;
    legacy.close().await?;

    let outbox = FleetOutbox::open(&path).await?;
    let migrated_settlement: i64 =
        sqlx::query_scalar("SELECT settled_at_ms FROM fleet_outbox_requests WHERE request_id=?")
            .bind(finished_request.request_id.as_str())
            .fetch_one(&outbox.database.pool)
            .await?;
    assert!(migrated_settlement > 0);
    assert_eq!(outbox.prune_expired(migrated_settlement, 500).await?, 0);
    assert!(
        outbox
            .request(&finished_request.request_id)
            .await?
            .is_some()
    );
    assert!(
        outbox
            .creation_input(&finished_request.request_id)
            .await?
            .is_some()
    );
    let after_two_months = migrated_settlement + 100 * 24 * 60 * 60 * 1_000;
    assert_eq!(outbox.prune_expired(after_two_months, 500).await?, 1);
    assert!(
        outbox
            .request(&finished_request.request_id)
            .await?
            .is_none()
    );
    assert!(
        outbox
            .creation_input(&finished_request.request_id)
            .await?
            .is_none()
    );
    assert!(outbox.request(&pending_request.request_id).await?.is_some());
    assert_eq!(
        outbox.creation_input(&pending_request.request_id).await?,
        Some(pending_input)
    );
    outbox.close().await;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn outbox_decoder_rejects_state_and_settlement_timestamp_mismatches() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let outbox = FleetOutbox::open(&temporary.path().join("outbox.sqlite")).await?;
    let (_, pending) = inputs()?;
    outbox.enqueue(&pending).await?;
    sqlx::query::<sqlx_turso::Turso>(
        "UPDATE fleet_outbox_requests SET settled_at_ms=1000 WHERE request_id=?",
    )
    .bind(pending.request_id.as_str())
    .execute(&outbox.database.pool)
    .await?;
    assert!(matches!(
        outbox.request(&pending.request_id).await,
        Err(RouterLocalStorageError::InvalidOutboxRetentionState { request_id })
            if request_id == pending.request_id.as_str()
    ));

    let (_, decided_without_time) = inputs()?;
    outbox.enqueue(&decided_without_time).await?;
    sqlx::query::<sqlx_turso::Turso>("UPDATE fleet_outbox_requests SET state=? WHERE request_id=?")
        .bind(serde_json::to_string(&FleetOutboxState::Decided {
            decision: accepted_decision()?,
        })?)
        .bind(decided_without_time.request_id.as_str())
        .execute(&outbox.database.pool)
        .await?;
    assert!(matches!(
        outbox.request(&decided_without_time.request_id).await,
        Err(RouterLocalStorageError::InvalidOutboxRetentionState { request_id })
            if request_id == decided_without_time.request_id.as_str()
    ));
    outbox.close().await;
    Ok(())
}
