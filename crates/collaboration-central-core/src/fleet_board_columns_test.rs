//! Canonical boards expose the approved body and envelope fields as typed columns.
use super::workspace_columns_tests::{mutate, prepare, workspace, write};
use crate::fleet_storage_fixture::{FleetStorageFixture, NOW, TestError};
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use message_board::{Board, BoardId, BoardKind, BoardState, Record};
use sqlx_core::{query::query, row::Row};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Board position boundaries and exact optional actor/envelope fields must be stored without coercion."
)]
async fn board_position_boundaries_and_precise_session_envelopes_survive_reopen()
-> Result<(), TestError> {
    for position in [0, i64::MAX as u64] {
        let fixture = seeded().await?;
        let mut offered = board(BoardKind::Area {}, BoardState::Open)?;
        let Record::Board(value) = &mut offered else {
            return Err("wrong fixture kind".into());
        };
        let message_board::RecordPlacement::Fleet(placement) = &mut value.envelope.position else {
            return Err("wrong fixture placement".into());
        };
        placement.position = message_board::FleetPosition::try_from(position)?;
        value.envelope.request = None;
        value.envelope.actor = message_board::RecordedActor::Session {
            session: message_board::SessionRef {
                endpoint: message_board::SessionEndpointRef {
                    service_id: "019f0000-0000-7000-8000-000000009e05"
                        .to_owned()
                        .try_into()?,
                    endpoint_id: "codex-local".to_owned().try_into()?,
                },
                session_id: "precise Board actor".to_owned().try_into()?,
            },
        };
        value.envelope.time =
            message_board::RecordTime::Router("2026-11-04T10:11:12.123456789Z".parse()?);
        write(&fixture, offered.clone(), NOW.unix_millis()).await?;
        assert_eq!(read(&fixture).await?, offered);
        let fixture = fixture.reopen().await?;
        assert_eq!(read(&fixture).await?, offered);
        fixture.close().await?;
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A real late constraint error must restore all Board columns, including its active kind fields."
)]
async fn late_board_write_failure_restores_original_kind_body_and_envelope() -> Result<(), TestError>
{
    let fixture = seeded().await?;
    let original = board(BoardKind::Area {}, BoardState::Open)?;
    write(&fixture, original.clone(), NOW.unix_millis()).await?;
    mutate(&fixture,"CREATE TABLE board_late_fault(row_id TEXT NOT NULL PRIMARY KEY,created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL); INSERT INTO board_late_fault VALUES('019f0000-0000-7000-8000-000000009e09',0,0)").await?;
    let failure=fixture.object.transaction(|scope|Box::pin(async move {
        let changed=board(BoardKind::Initiative {goal:"rolled-back goal".into(),done_when:"rolled-back finish".into()},BoardState::Closed).map_err(|error|StoreError::backend(error.to_string()))?;
        let digest=fleet_admission::FleetImportDigest::new("ab".repeat(32)).map_err(StoreError::backend)?;
        let mut ids=crate::fleet_row_ids::FleetRowIds::new(std::sync::Arc::new(crate::fleet_storage_fixture::FixtureIo));
        crate::fleet_record_store::put(scope,&changed,crate::fleet_record_store::RecordWriteSource::Imported(&digest),chrono::DateTime::from_timestamp_millis(NOW.unix_millis()+17).ok_or_else(||StoreError::backend("invalid clock"))?,&mut ids).await?;
        query::<sqlx_turso::Turso>("INSERT INTO board_late_fault VALUES('019f0000-0000-7000-8000-000000009e09',0,0)").execute(scope.executor()).await.map_err(StoreError::backend)?;
        Ok(())
    })).await;
    assert!(failure.is_err());
    assert_eq!(read(&fixture).await?, original);
    let fixture = fixture.reopen().await?;
    assert_eq!(read(&fixture).await?, original);
    fixture.close().await?;
    Ok(())
}

fn board(kind: BoardKind, state: BoardState) -> Result<Record, TestError> {
    let Record::Workspace(parent) = workspace(1)? else {
        return Err("wrong fixture kind".into());
    };
    let id: BoardId = "019f0000-0000-7000-8000-000000009e08"
        .to_owned()
        .try_into()?;
    let mut envelope = parent.envelope;
    envelope.id = id.clone().into();
    envelope.scope.board = Some(id.clone());
    Ok(Record::Board(Board {
        envelope,
        board_id: id,
        workspace_id: parent.workspace_id,
        name: "typed-board".to_owned().try_into()?,
        description: "board body".to_owned().try_into()?,
        kind,
        state,
    }))
}

async fn seeded() -> Result<FleetStorageFixture, TestError> {
    let fixture = prepare().await?;
    write(&fixture, workspace(1)?, NOW.unix_millis()).await?;
    Ok(fixture)
}

async fn read(fixture: &FleetStorageFixture) -> Result<Record, StoreError> {
    fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                let reference =
                    BoardId::try_from("019f0000-0000-7000-8000-000000009e08".to_owned())
                        .map_err(StoreError::backend)?
                        .into();
                crate::fleet_record_store::read(scope, &reference)
                    .await?
                    .map(|stored| stored.record)
                    .ok_or_else(|| StoreError::backend("board missing"))
            })
        })
        .await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The literal owner-approved board column list rejects an unchanged JSON schema."
)]
async fn board_schema_contains_only_approved_typed_body_and_envelope_columns()
-> Result<(), TestError> {
    let fixture = prepare().await?;
    let proof = async {
        let mut columns: Vec<(String, String, i64)> = fixture
            .object
            .transaction(|scope| {
                Box::pin(async move {
                    query::<sqlx_turso::Turso>("PRAGMA table_info(fleet_boards)")
                        .fetch_all(scope.executor())
                        .await
                        .map_err(StoreError::backend)?
                        .into_iter()
                        .map(|row| {
                            Ok((
                                row.try_get("name").map_err(StoreError::backend)?,
                                row.try_get("type").map_err(StoreError::backend)?,
                                row.try_get("pk").map_err(StoreError::backend)?,
                            ))
                        })
                        .collect()
                })
            })
            .await?;
        let mut expected = [
            ("board_id", "TEXT", 1),
            ("workspace_id", "TEXT", 0),
            ("name", "TEXT", 0),
            ("description", "TEXT", 0),
            ("board_kind", "TEXT", 0),
            ("initiative_goal", "TEXT", 0),
            ("initiative_done_when", "TEXT", 0),
            ("state", "TEXT", 0),
            ("actor_kind", "TEXT", 0),
            ("actor_human_id", "TEXT", 0),
            ("actor_service_id", "TEXT", 0),
            ("actor_endpoint_id", "TEXT", 0),
            ("actor_session_id", "TEXT", 0),
            ("actor_router_component", "TEXT", 0),
            ("request_id", "TEXT", 0),
            ("fleet_position", "INTEGER", 0),
            ("router_time", "TEXT", 0),
            ("time_basis", "TEXT", 0),
            ("origin_installation", "TEXT", 0),
            ("origin_machine", "TEXT", 0),
            ("import_digest", "TEXT", 0),
            ("created_at", "INTEGER", 0),
            ("updated_at", "INTEGER", 0),
        ]
        .into_iter()
        .map(|(name, kind, pk)| (name.to_owned(), kind.to_owned(), pk))
        .collect::<Vec<_>>();
        columns.sort_unstable();
        expected.sort_unstable();
        assert_eq!(
            columns, expected,
            "Board body and complete envelope must be typed, with no payload column"
        );
        Ok::<_, TestError>(())
    }
    .await;
    let closed = fixture.close().await;
    closed?;
    proof
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Every declared Board kind/state and original creation time must survive owning reopen."
)]
async fn board_variants_states_and_first_timestamps_survive_rename_and_reopen()
-> Result<(), TestError> {
    for kind in [
        BoardKind::Area {},
        BoardKind::Initiative {
            goal: "\0".repeat(32 * 1024),
            done_when: "\0".repeat(32 * 1024),
        },
    ] {
        for state in [
            BoardState::Open,
            BoardState::Closed,
            BoardState::Archiving,
            BoardState::Archived,
        ] {
            let fixture = seeded().await?;
            let offered = board(kind.clone(), state)?;
            write(&fixture, offered.clone(), NOW.unix_millis()).await?;
            assert_eq!(read(&fixture).await?, offered);
            let fixture = fixture.reopen().await?;
            assert_eq!(read(&fixture).await?, offered);
            let mut renamed = offered;
            let Record::Board(value) = &mut renamed else {
                return Err("wrong fixture kind".into());
            };
            value.name = "renamed-board".to_owned().try_into()?;
            write(&fixture, renamed.clone(), NOW.unix_millis() + 17).await?;
            assert_eq!(read(&fixture).await?, renamed);
            let times = fixture
                .object
                .transaction(|scope| {
                    Box::pin(async move {
                        let row = query::<sqlx_turso::Turso>(
                            "SELECT created_at,updated_at FROM fleet_boards",
                        )
                        .fetch_one(scope.executor())
                        .await
                        .map_err(StoreError::backend)?;
                        Ok((
                            row.try_get::<i64, _>("created_at")
                                .map_err(StoreError::backend)?,
                            row.try_get::<i64, _>("updated_at")
                                .map_err(StoreError::backend)?,
                        ))
                    })
                })
                .await?;
            assert_eq!(times, (NOW.unix_millis(), NOW.unix_millis() + 17));
            fixture.close().await?;
        }
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Unknown tags, invalid physical classes and unused kind fields must fail the actual Board decoder."
)]
async fn board_rejects_unknown_tags_classes_and_malformed_initiative_variants()
-> Result<(), TestError> {
    for sql in [
        "UPDATE fleet_boards SET board_kind='unknown'",
        "UPDATE fleet_boards SET state='unknown'",
        "UPDATE fleet_boards SET state=x'6f70656e'",
        "UPDATE fleet_boards SET initiative_goal='unused'",
        "UPDATE fleet_boards SET board_kind='initiative',initiative_goal='goal'",
        "UPDATE fleet_boards SET actor_kind='unknown'",
        "UPDATE fleet_boards SET actor_router_component='archive_observation'",
        "UPDATE fleet_boards SET name=' padded '",
        "UPDATE fleet_boards SET fleet_position=1.5",
        "UPDATE fleet_boards SET created_at=x'3132'",
    ] {
        let fixture = seeded().await?;
        write(
            &fixture,
            board(BoardKind::Area {}, BoardState::Open)?,
            NOW.unix_millis(),
        )
        .await?;
        mutate(&fixture, sql).await?;
        assert!(
            matches!(read(&fixture).await, Err(StoreError::Corrupt { .. })),
            "invalid Board accepted: {sql}"
        );
        fixture.close().await?;
    }
    Ok(())
}
