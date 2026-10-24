//! Public MCP replies come from actual persisted library authority, without local grants.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
use board_fixture::{BoardFixture, ControlledClock, TestError, accepted, fixture_peer};
use fleet_sync_protocol::{
    DatabaseId, Epoch, LeaseRefusal, LeaseState, LeaseStatus, RenewAnswer, RenewResult,
};
use message_board::*;
use runtime_primitives::{Clock, SystemClock};
use serde_json::{Value, json};
use std::sync::Arc;

#[allow(clippy::panic_in_result_fn)] // These are the gate's unchanged native-state assertions.
async fn assert_unrecorded(
    fixture: &BoardFixture,
    project: &ProjectId,
    request: &RequestId,
    task: &TaskId,
    high: FeedCursor,
) -> Result<(), TestError> {
    let store = fixture
        ._registry
        .existing_project(project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    assert_eq!(read.high_water(), high);
    assert!(read.record(&request.clone().into()).await?.is_none());
    assert!(read.record(&task.clone().into()).await?.is_none());
    assert!(read.records(RecordKind::ReadMark).await?.is_empty());
    read.finish().await?;
    Ok(())
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn actual_expiry_names_the_deadline_keeps_reads_and_same_epoch_renewal_admits_the_same_key()
-> Result<(), TestError> {
    let clock = Arc::new(ControlledClock::at(SystemClock.now()));
    let initial = clock.now();
    let fixture = BoardFixture::start_with_clock(clock.clone()).await?;
    let (project, workstream) = fixture.project().await?;
    let task = TaskId::generate();
    let request = fixture.request(&project, json!({"kind":"taskCreate", "taskId":task,
        "workstreamId":workstream, "title":"after renewal", "body":"", "blockedBy":[], "relatedTo":[]}));
    let request_id: RequestId =
        serde_json::from_value(request.get("requestId").ok_or("request key")?.clone())?;
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let read = store.snapshot().await?;
    let high = read.high_water();
    read.finish().await?;
    let database = fixture._registry.database(&project).await?;
    let expires_at = initial.after(board_fleet_sync::BOARD_LEASE_POLICY.lease_duration);
    let lease = LeaseStatus {
        database: DatabaseId::new(uuid::Uuid::parse_str(project.as_str())?)?,
        epoch: Epoch::new(1)?,
        holder: fixture_peer()?,
        state: LeaseState::Held,
        expires_at,
        remaining_ms: 14_400_000,
    };
    clock.set(expires_at);
    let expected_expiry =
        chrono::DateTime::from_timestamp_millis(expires_at.unix_millis()).ok_or("expiry UTC")?;
    let response = fixture.api.call("task_create", request.clone()).await?;
    assert_eq!(
        response.pointer("/result/result"),
        Some(&serde_json::to_value(RequestResult::LeaseExpired {
            expired_at: expected_expiry
        })?),
        "{response}"
    );
    let read: Value = fixture.api.call("feed_read", json!({"projectId":project,
        "reader":{"kind":"identified","reader":fixture.actor}, "after":{"epoch":1,"position":0}, "limit":100, "includeRedacted":false})).await?;
    assert_eq!(
        read.pointer("/result/readMark"),
        Some(&serde_json::to_value(ReadMarkOutcome::NotAdvanced {
            reason: ReadMarkNotAdvanced::LeaseExpired {
                expired_at: expected_expiry
            },
        })?),
        "{read}"
    );
    assert!(
        read.pointer("/result/value/entries")
            .and_then(Value::as_array)
            .is_some_and(|entries| !entries.is_empty()),
        "expiry hid readable records: {read}"
    );
    assert_unrecorded(&fixture, &project, &request_id, &task, high).await?;
    let renewed_at = clock.now();
    let renewed_until = renewed_at.after(board_fleet_sync::BOARD_LEASE_POLICY.lease_duration);
    database
        .observe_renewal(
            RenewAnswer::Renewed {
                recorded: RenewResult {
                    epoch: lease.epoch,
                    expires_at: renewed_until,
                },
                current: LeaseStatus {
                    expires_at: renewed_until,
                    remaining_ms: 14_400_000,
                    ..lease
                },
                transfer_in_progress: None,
                replayed: false,
            },
            renewed_at,
        )
        .await?;
    let first = fixture.api.call("task_create", request.clone()).await?;
    accepted(&first);
    let mut read = store.snapshot().await?;
    let accepted_high = read.high_water();
    assert_eq!(accepted_high.epoch, high.epoch);
    let Some(Record::Request(ledger)) = read.record(&request_id.into()).await? else {
        return Err("accepted ledger".into());
    };
    assert_eq!(ledger.accepted_in, Some(EpochNumber::INITIAL));
    assert!(matches!(ledger.decision, StoredDecision::Accepted { .. }));
    read.finish().await?;
    assert_eq!(
        fixture
            .api
            .call("task_create", request)
            .await?
            .pointer("/result/result"),
        first.pointer("/result/result")
    );
    let read = store.snapshot().await?;
    assert_eq!(read.high_water(), accepted_high, "replay appended activity");
    read.finish().await?;
    drop(database);
    drop(store);
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn an_actual_superseding_renewal_keeps_the_write_refusal_and_stops_public_reads()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let task = TaskId::generate();
    let request = fixture.request(&project, json!({"kind":"taskCreate", "taskId":task,
        "workstreamId":workstream, "title":"non-holder", "body":"", "blockedBy":[], "relatedTo":[]}));
    let request_id: RequestId =
        serde_json::from_value(request.get("requestId").ok_or("request key")?.clone())?;
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let read = store.snapshot().await?;
    let high = read.high_water();
    read.finish().await?;
    let database = fixture._registry.database(&project).await?;
    let now = SystemClock.now();
    let move_check = json!({"projectId":project,"actor":fixture.actor,
        "moveName":"missing","expectedRevision":0});
    let before = fixture.api.call("move_check", move_check.clone()).await?;
    assert_eq!(
        before.pointer("/result/kind"),
        Some(&json!("rejected")),
        "{before}"
    );
    database
        .observe_renewal(
            RenewAnswer::Refused(LeaseRefusal::NotWriter {
                current: Some(Epoch::new(2)?),
            }),
            now,
        )
        .await?;
    let response = fixture.api.call("task_create", request).await?;
    assert_eq!(
        response.pointer("/result/result"),
        Some(&serde_json::to_value(RequestResult::NotWriter {
            current: Some(EpochNumber::try_from(2)?)
        })?),
        "{response}"
    );
    assert_eq!(
        response.pointer("/result/reader"),
        Some(&Value::Null),
        "{response}"
    );
    let read = fixture.api.call("feed_read", json!({"projectId":project,
        "reader":{"kind":"identified","reader":fixture.actor}, "after":{"epoch":1,"position":0}, "limit":100, "includeRedacted":false})).await?;
    assert_eq!(
        read.pointer("/error/data/kind"),
        Some(&json!("boardUnavailable")),
        "{read}"
    );
    assert!(
        read.get("result").is_none(),
        "superseded records were served: {read}"
    );
    let checked = fixture.api.call("move_check", move_check).await?;
    assert_eq!(
        checked.pointer("/error/data/kind"),
        Some(&json!("boardUnavailable")),
        "{checked}"
    );
    assert!(
        checked.get("result").is_none(),
        "superseded move check was served: {checked}"
    );
    assert_unrecorded(&fixture, &project, &request_id, &task, high).await?;
    drop(database);
    drop(store);
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn an_unregistered_native_owner_refuses_the_first_write_and_registration_enables_it()
-> Result<(), TestError> {
    // Arrange an empty native file as a component input. This does not exercise
    // the central-first creation API or stand in for its directory acceptance.
    let fixture = BoardFixture::start().await?;
    let project = ProjectId::generate();
    let store = fixture._registry.open_project(&project).await?;
    let before = store.snapshot().await?;
    let empty = before.high_water();
    assert_eq!(empty.position, FeedPosition::ZERO);
    before.finish().await?;

    // Act: even the initial project row must wait for a real registration state.
    match store.begin_admission(fixture.record_origin()?).await {
        Err(message_board_storage::ProjectStorageError::WriteRefused(
            fleet_sync_writer::WriteRefusal::Provisional,
        )) => {}
        Err(error) => return Err(format!("unexpected unregistered refusal: {error}").into()),
        Ok(write) => {
            write.rollback().await?;
            return Err("the unregistered native owner admitted the first write".into());
        }
    }
    let database = fixture._registry.database(&project).await?;
    let now = SystemClock.now();
    database
        .observe_registration(
            LeaseStatus {
                database: DatabaseId::new(uuid::Uuid::parse_str(project.as_str())?)?,
                epoch: Epoch::new(1)?,
                holder: fixture_peer()?,
                state: LeaseState::Held,
                expires_at: now.after(board_fleet_sync::BOARD_LEASE_POLICY.lease_duration),
                remaining_ms: 14_400_000,
            },
            now,
        )
        .await?;
    let mut write = store.begin_admission(fixture.record_origin()?).await?;
    let cursor = write
        .append_record(Record::Project(ProjectState {
            envelope: RecordEnvelope {
                id: project.clone().into(),
                scope: RecordScope {
                    workspace: fixture.workspace.clone(),
                    board: Some(fixture.board.clone()),
                    project: Some(project.clone()),
                },
                actor: RecordedActor::declared(serde_json::from_value(fixture.actor.clone())?),
                request: None,
                position: RecordPlacement::Project(ProjectFeedEntry {
                    cursor: empty,
                    origin: fixture.record_origin()?,
                }),
                time: RecordTime::Router(chrono::Utc::now()),
            },
            project_id: project.clone(),
            description: Description::try_from("first registered write".to_owned())?,
            status: None,
            pinned_definition: None,
            revision: SubjectRevision::INITIAL,
            closed_at: None,
        }))
        .await?;
    write.commit().await?;

    // Assert the refused attempt added nothing and the registered write is first.
    assert_eq!(cursor.epoch, EpochNumber::INITIAL);
    assert_eq!(cursor.position.get(), 1);
    let mut read = store.snapshot().await?;
    assert_eq!(read.high_water(), cursor);
    assert!(
        matches!(read.record(&project.into()).await?, Some(Record::Project(state))
        if state.description.as_str() == "first registered write")
    );
    read.finish().await?;
    drop(database);
    drop(store);
    fixture.stop().await
}
