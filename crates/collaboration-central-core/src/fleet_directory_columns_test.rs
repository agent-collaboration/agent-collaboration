//! The directory's identity and committed Copy/lease projections have separate typed fields.
use super::workspace_columns_tests::prepare;
use crate::fleet_storage_fixture::TestError;
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use sqlx_core::{query::query, row::Row};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The approved directory columns must exist physically and replace every JSON projection."
)]
async fn directory_schema_exposes_identity_envelope_copy_and_lease_columns() -> Result<(), TestError>
{
    let fixture = prepare().await?;
    let columns: Vec<(String, String, i64)> = fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                query::<sqlx_turso::Turso>("PRAGMA table_info(fleet_project_directory)")
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
    fixture.close().await?;
    let expected = [
        "project_id",
        "workspace_id",
        "board_id",
        "name",
        "actor_kind",
        "actor_human_id",
        "actor_service_id",
        "actor_endpoint_id",
        "actor_session_id",
        "actor_router_component",
        "request_id",
        "fleet_position",
        "router_time",
        "time_basis",
        "origin_installation",
        "origin_machine",
        "import_digest",
        "created_at",
        "updated_at",
        "copy_epoch",
        "copy_sequence",
        "copy_acked_at",
        "copy_latest_time_basis",
        "copy_latest_record_at",
        "lease_epoch",
        "lease_change",
        "lease_incarnation",
        "lease_changed_at",
        "lease_lifecycle_kind",
        "lease_origin_kind",
        "lease_origin_epoch",
        "lease_origin_sequence",
        "lease_import_digest",
        "lease_retiring_attempt_id",
        "lease_retired_at",
        "lease_holder_machine_id",
        "lease_holder_service_id",
        "lease_expires_at",
        "lease_digest",
    ];
    assert_eq!(
        columns.len(),
        expected.len(),
        "directory schema: {columns:?}"
    );
    for name in expected {
        let storage = if [
            "fleet_position",
            "created_at",
            "updated_at",
            "copy_acked_at",
            "lease_changed_at",
            "lease_retired_at",
            "lease_expires_at",
        ]
        .contains(&name)
        {
            "INTEGER"
        } else {
            "TEXT"
        };
        assert!(
            columns.contains(&(name.into(), storage.into(), i64::from(name == "project_id"))),
            "missing {name}: {columns:?}"
        );
    }
    Ok(())
}

pub(super) const PROJECT: &str = "019f0000-0000-7000-8000-000000009f12";
const BOARD: &str = "019f0000-0000-7000-8000-000000009f11";

fn project() -> Result<message_board::Record, TestError> {
    use message_board::{ProjectDirectoryEntry, Record};
    let Record::Workspace(parent) = super::workspace_columns_tests::workspace(1)? else {
        return Err("wrong fixture kind".into());
    };
    let id: message_board::ProjectId = PROJECT.to_owned().try_into()?;
    let board: message_board::BoardId = BOARD.to_owned().try_into()?;
    let mut envelope = parent.envelope;
    envelope.id = id.clone().into();
    envelope.scope.board = Some(board.clone());
    Ok(Record::DirectoryEntry(ProjectDirectoryEntry {
        envelope,
        id,
        board,
        title: "literal-project".to_owned().try_into()?,
    }))
}

pub(super) async fn seeded() -> Result<crate::fleet_storage_fixture::FleetStorageFixture, TestError>
{
    use super::workspace_columns_tests::{workspace, write};
    use crate::fleet_storage_fixture::NOW;
    use message_board::{Board, BoardKind, BoardState, Record};
    let fixture = prepare().await?;
    let parent = workspace(1)?;
    let Record::Workspace(workspace) = &parent else {
        return Err("wrong fixture kind".into());
    };
    let id: message_board::BoardId = BOARD.to_owned().try_into()?;
    let mut envelope = workspace.envelope.clone();
    envelope.id = id.clone().into();
    envelope.scope.board = Some(id.clone());
    let board = Record::Board(Board {
        envelope,
        board_id: id,
        workspace_id: workspace.workspace_id.clone(),
        name: "directory-board".to_owned().try_into()?,
        description: "directory fixture".to_owned().try_into()?,
        state: BoardState::Open,
        kind: BoardKind::Area {},
    });
    write(&fixture, parent, NOW.unix_millis()).await?;
    write(&fixture, board, NOW.unix_millis()).await?;
    write(&fixture, project()?, NOW.unix_millis()).await?;
    Ok(fixture)
}

async fn read(
    fixture: &crate::fleet_storage_fixture::FleetStorageFixture,
) -> Result<message_board::Record, StoreError> {
    fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                crate::fleet_directory_store::read(scope, PROJECT)
                    .await?
                    .map(|stored| stored.record)
                    .ok_or_else(|| StoreError::backend("directory missing"))
            })
        })
        .await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The directory must preserve its literal full envelope and first ingestion time across reopen and rename."
)]
async fn directory_precise_envelope_position_extremes_and_first_time_survive_reopen()
-> Result<(), TestError> {
    use super::workspace_columns_tests::write;
    use crate::fleet_storage_fixture::NOW;
    for position in [0, i64::MAX as u64] {
        let fixture = seeded().await?;
        let mut offered = project()?;
        let message_board::Record::DirectoryEntry(value) = &mut offered else {
            return Err("wrong fixture kind".into());
        };
        value.envelope.time =
            message_board::RecordTime::Router("2026-11-04T10:11:12.123456789Z".parse()?);
        value.envelope.request = None;
        value.envelope.actor = message_board::RecordedActor::Session {
            session: message_board::SessionRef {
                endpoint: message_board::SessionEndpointRef {
                    service_id: "019f0000-0000-7000-8000-000000009f13"
                        .to_owned()
                        .try_into()?,
                    endpoint_id: "codex-local".to_owned().try_into()?,
                },
                session_id: "literal session".to_owned().try_into()?,
            },
        };
        let message_board::RecordPlacement::Fleet(placement) = &mut value.envelope.position else {
            return Err("wrong fixture position".into());
        };
        placement.position = message_board::FleetPosition::try_from(position)?;
        write(&fixture, offered.clone(), NOW.unix_millis() + 17).await?;
        assert_eq!(read(&fixture).await?, offered);
        let fixture = fixture.reopen().await?;
        assert_eq!(read(&fixture).await?, offered);
        let times = fixture
            .object
            .transaction(|scope| {
                Box::pin(async move {
                    let row = query::<sqlx_turso::Turso>(
                        "SELECT created_at,updated_at FROM fleet_project_directory",
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
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The canonical typed directory decoder must reject corrupt envelope variants and physical storage values."
)]
async fn directory_rejects_unknown_envelopes_wrong_classes_and_noncanonical_values()
-> Result<(), TestError> {
    use super::workspace_columns_tests::mutate;
    for sql in [
        "UPDATE fleet_project_directory SET actor_kind='unknown'",
        "UPDATE fleet_project_directory SET actor_kind=x'68756d616e'",
        "UPDATE fleet_project_directory SET actor_endpoint_id='codex-local'",
        "UPDATE fleet_project_directory SET name=' padded '",
        "UPDATE fleet_project_directory SET name=x'6e616d65'",
        "UPDATE fleet_project_directory SET fleet_position=-1",
        "UPDATE fleet_project_directory SET fleet_position=x'31'",
        "UPDATE fleet_project_directory SET request_id='019f0000-0000-4000-8000-000000009f12'",
        "UPDATE fleet_project_directory SET router_time='invalid'",
        "UPDATE fleet_project_directory SET time_basis='unknown'",
        "UPDATE fleet_project_directory SET time_basis='router',router_time=NULL",
        "UPDATE fleet_project_directory SET import_digest=NULL",
        "UPDATE fleet_project_directory SET created_at=x'31'",
        "UPDATE fleet_project_directory SET origin_installation='not-a-uuid'",
        "UPDATE fleet_project_directory SET project_id='019f0000-0000-4000-8000-000000009f12'",
    ] {
        let fixture = seeded().await?;
        mutate(&fixture, sql).await?;
        // Query the row's current physical key too, so a corrupt PK is decoded rather than missed.
        let result = fixture
            .object
            .transaction(|scope| {
                Box::pin(async move {
                    let rows = crate::generated_fleet_record_reads::list_board_projects(
                        scope,
                        BOARD.into(),
                    )
                    .await
                    .map_err(StoreError::backend)?;
                    rows.into_iter()
                        .map(crate::fleet_directory_codec::decode)
                        .collect::<Result<Vec<_>, _>>()
                })
            })
            .await;
        assert!(result.is_err(), "{sql}");
        fixture.close().await?;
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A real late SQL failure must roll back directory identity and both committed projections as one Fleet transaction."
)]
async fn late_directory_transaction_failure_restores_identity_copy_and_lease_across_reopen()
-> Result<(), TestError> {
    use super::workspace_columns_tests::mutate;
    use crate::{fleet_record_codec::FleetRecordHeader, fleet_storage_fixture::NOW};
    use message_board::Record;
    let fixture = seeded().await?;
    let original = read(&fixture).await?;
    mutate(&fixture,"CREATE TABLE directory_late_fault(row_id TEXT PRIMARY KEY,created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL); INSERT INTO directory_late_fault VALUES('019f0000-0000-7000-8000-000000009f19',0,0)").await?;
    let failure=fixture.object.transaction(|scope|Box::pin(async move {
        let mut offered=project().map_err(|error|StoreError::backend(error.to_string()))?;
        let Record::DirectoryEntry(value)=&mut offered else {return Err(StoreError::backend("wrong fixture kind"));};
        value.title="rolled-back project".to_owned().try_into().map_err(StoreError::backend)?;
        let header=FleetRecordHeader::for_record(&offered)?;
        let Record::DirectoryEntry(value)=&offered else {return Err(StoreError::backend("wrong fixture kind"));};
        let clock=chrono::DateTime::from_timestamp_millis(NOW.unix_millis()+17).ok_or_else(||StoreError::backend("invalid fixture clock"))?;
        crate::fleet_directory_store::put(scope,value,&header,Some("ab".repeat(32)),clock).await?;
        crate::generated_fleet_record_writes::store_directory_summary(scope,Some("1".into()),Some("9".into()),Some("17".into()),Some("legacy_unknown".into()),None,clock.timestamp_millis().to_string(),PROJECT.into()).await.map_err(StoreError::backend)?;
        // The typed lease write stays in this same scope; its digest is validated on every read.
        crate::generated_fleet_lease::store_directory_lease(scope,Some("1".into()),Some("1".into()),Some("1".into()),Some("17".into()),Some("retiring".into()),None,None,None,None,Some("019f0000-0000-7000-8000-000000009f15".into()),None,None,None,None,Some("ab".repeat(32)),clock.timestamp_millis().to_string(),PROJECT.into()).await.map_err(StoreError::backend)?;
        query::<sqlx_turso::Turso>("INSERT INTO directory_late_fault VALUES('019f0000-0000-7000-8000-000000009f19',0,0)").execute(scope.executor()).await.map_err(StoreError::backend)?;
        Ok(())
    })).await;
    assert!(failure.is_err());
    assert!(
        format!("{failure:?}").contains("directory_late_fault"),
        "wrong failure: {failure:?}"
    );
    let fixture = fixture.reopen().await?;
    assert_eq!(read(&fixture).await?, original);
    let projections = fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                let copy = crate::fleet_copy_projection::decode(
                    crate::generated_fleet_record_reads::read_directory_summary(
                        scope,
                        PROJECT.into(),
                    )
                    .await
                    .map_err(StoreError::backend)?
                    .ok_or_else(|| StoreError::backend("directory missing"))?,
                )?;
                let lease = crate::fleet_lease_columns::decode(
                    crate::generated_fleet_lease::read_directory_lease(scope, PROJECT.into())
                        .await
                        .map_err(StoreError::backend)?
                        .ok_or_else(|| StoreError::backend("directory missing"))?,
                )?;
                Ok((copy, lease))
            })
        })
        .await?;
    assert_eq!(projections, (None, None));
    fixture.close().await?;
    Ok(())
}
