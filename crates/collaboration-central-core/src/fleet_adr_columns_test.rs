//! Canonical ADR fields and ordered repository files use typed owner-approved columns.
use super::workspace_columns_tests::{mutate, prepare, workspace, write};
use crate::fleet_storage_fixture::{FleetStorageFixture, NOW, TestError};
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use message_board::{
    Adr, AdrId, AdrStatus, ArtifactRef, Record, RecordKind, RecordRef, SubjectRevision,
};
use sqlx_core::{query::query, row::Row};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A supersedes identity and the remaining lifecycle states must retain their precise supplied meaning through reopen."
)]
async fn adr_supersedes_reference_and_terminal_lifecycle_states_survive_reopen()
-> Result<(), TestError> {
    for status in [AdrStatus::Superseded, AdrStatus::Withdrawn] {
        let fixture = seeded().await?;
        let previous: AdrId = "019f0000-0000-7000-8000-000000009e12"
            .to_owned()
            .try_into()?;
        let mut predecessor = adr(0)?;
        let Record::Adr(value) = &mut predecessor else {
            return Err("wrong fixture kind".into());
        };
        value.adr_id = previous.clone();
        value.envelope.id = previous.clone().into();
        value.repository_files.clear();
        write(&fixture, predecessor, NOW.unix_millis()).await?;
        let mut offered = adr(7)?;
        let Record::Adr(value) = &mut offered else {
            return Err("wrong fixture kind".into());
        };
        value.status = status;
        value.supersedes = Some(previous);
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
    reason = "A real late SQL error must restore ADR parent columns and every ordered file row."
)]
async fn late_adr_write_failure_rolls_back_parent_and_ordered_repository_files()
-> Result<(), TestError> {
    let fixture = seeded().await?;
    let original = adr(1)?;
    write(&fixture, original.clone(), NOW.unix_millis()).await?;
    mutate(&fixture,"CREATE TABLE adr_late_fault(row_id TEXT NOT NULL PRIMARY KEY,created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL); INSERT INTO adr_late_fault VALUES('019f0000-0000-7000-8000-000000009e13',0,0)").await?;
    let failure = fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                let mut changed = adr(2).map_err(|error| StoreError::backend(error.to_string()))?;
                let Record::Adr(value) = &mut changed else {
                    return Err(StoreError::backend("wrong fixture kind"));
                };
                value.title = "rolled back title"
                    .to_owned()
                    .try_into()
                    .map_err(StoreError::backend)?;
                value.status = AdrStatus::Accepted;
                value.repository_files.reverse();
                value.repository_files.pop();
                let digest = fleet_admission::FleetImportDigest::new("ab".repeat(32))
                    .map_err(StoreError::backend)?;
                let mut ids = crate::fleet_row_ids::FleetRowIds::new(std::sync::Arc::new(
                    crate::fleet_storage_fixture::FixtureIo,
                ));
                crate::fleet_record_store::put(
                    scope,
                    &changed,
                    crate::fleet_record_store::RecordWriteSource::Imported(&digest),
                    chrono::DateTime::from_timestamp_millis(NOW.unix_millis() + 17)
                        .ok_or_else(|| StoreError::backend("invalid clock"))?,
                    &mut ids,
                )
                .await?;
                query::<sqlx_turso::Turso>(
                    "INSERT INTO adr_late_fault VALUES('019f0000-0000-7000-8000-000000009e13',0,0)",
                )
                .execute(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                Ok(())
            })
        })
        .await;
    assert!(failure.is_err());
    assert_eq!(read(&fixture).await?, original);
    let fixture = fixture.reopen().await?;
    assert_eq!(read(&fixture).await?, original);
    fixture.close().await?;
    Ok(())
}

fn adr(revision: u64) -> Result<Record, TestError> {
    let Record::Workspace(parent) = workspace(1)? else {
        return Err("wrong fixture kind".into());
    };
    let id: AdrId = "019f0000-0000-7000-8000-000000009e10"
        .to_owned()
        .try_into()?;
    let mut envelope = parent.envelope;
    envelope.id = id.clone().into();
    Ok(Record::Adr(Adr {
        envelope,
        adr_id: id,
        workspace: parent.workspace_id,
        title: "typed ADR".to_owned().try_into()?,
        text: "# Immutable ADR\nPreserve Markdown and λ"
            .to_owned()
            .try_into()?,
        status: AdrStatus::Proposed,
        supersedes: None,
        made_in: None,
        repository_files: (0..16)
            .map(|ordinal| {
                Ok(ArtifactRef {
                    repository: format!("example.test/repository-{ordinal}").try_into()?,
                    path: format!("\0file/{ordinal}"),
                    commit: if ordinal % 2 == 0 {
                        Some("a".repeat(40).try_into()?)
                    } else {
                        None
                    },
                })
            })
            .collect::<Result<Vec<_>, TestError>>()?,
        revision: SubjectRevision::try_from(revision)?,
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
                let reference = AdrId::try_from("019f0000-0000-7000-8000-000000009e10".to_owned())
                    .map_err(StoreError::backend)?
                    .into();
                crate::fleet_record_store::read(scope, &reference)
                    .await?
                    .map(|stored| stored.record)
                    .ok_or_else(|| StoreError::backend("ADR missing"))
            })
        })
        .await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Supplied immutable text and all sixteen ordered files must survive typed storage and owning reopen."
)]
async fn adr_revision_extremes_and_full_ordered_file_list_survive_transition_and_reopen()
-> Result<(), TestError> {
    for revision in [0, i64::MAX as u64] {
        let fixture = seeded().await?;
        let original = adr(revision)?;
        write(&fixture, original.clone(), NOW.unix_millis()).await?;
        assert_eq!(read(&fixture).await?, original);
        let fixture = fixture.reopen().await?;
        assert_eq!(read(&fixture).await?, original);
        let mut accepted = original;
        let Record::Adr(value) = &mut accepted else {
            return Err("wrong fixture kind".into());
        };
        value.status = AdrStatus::Accepted;
        write(&fixture, accepted.clone(), NOW.unix_millis() + 17).await?;
        assert_eq!(read(&fixture).await?, accepted);
        let times=fixture.object.transaction(|scope|Box::pin(async move{
            let row=query::<sqlx_turso::Turso>("SELECT created_at,updated_at,(SELECT MIN(created_at) FROM fleet_adr_repository_files) AS file_created,(SELECT MAX(updated_at) FROM fleet_adr_repository_files) AS file_updated FROM fleet_workspace_adrs").fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
            Ok((row.try_get::<i64,_>("created_at").map_err(StoreError::backend)?,row.try_get::<i64,_>("updated_at").map_err(StoreError::backend)?,row.try_get::<i64,_>("file_created").map_err(StoreError::backend)?,row.try_get::<i64,_>("file_updated").map_err(StoreError::backend)?))
        })).await?;
        assert_eq!(
            times,
            (
                NOW.unix_millis(),
                NOW.unix_millis() + 17,
                NOW.unix_millis(),
                NOW.unix_millis()
            )
        );
        let mut invalid = accepted.clone();
        let Record::Adr(value) = &mut invalid else {
            return Err("wrong fixture kind".into());
        };
        value.text = "different immutable text".to_owned().try_into()?;
        assert!(
            write(&fixture, invalid, NOW.unix_millis() + 99)
                .await
                .is_err()
        );
        assert_eq!(read(&fixture).await?, accepted);
        fixture.close().await?;
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The optional made-in reference accepts every shared record kind without changing its identity."
)]
async fn adr_made_in_accepts_the_full_shared_record_kind_vocabulary() -> Result<(), TestError> {
    for kind in [
        RecordKind::Workspace,
        RecordKind::Board,
        RecordKind::Adr,
        RecordKind::Project,
        RecordKind::Workstream,
        RecordKind::Task,
        RecordKind::TaskLinkEvent,
        RecordKind::Subtask,
        RecordKind::Thread,
        RecordKind::ThreadRefiled,
        RecordKind::Post,
        RecordKind::RoleAssignment,
        RecordKind::OwnerGrant,
        RecordKind::StandDownAcknowledgement,
        RecordKind::Fact,
        RecordKind::PlanRevision,
        RecordKind::ReadMark,
        RecordKind::NotifyIntent,
        RecordKind::SubscriptionMembership,
        RecordKind::Request,
        RecordKind::WorkTrailRow,
        RecordKind::TrailRedaction,
    ] {
        let fixture = seeded().await?;
        let mut offered = adr(0)?;
        let Record::Adr(value) = &mut offered else {
            return Err("wrong fixture kind".into());
        };
        value.repository_files.clear();
        value.made_in = Some(RecordRef {
            kind,
            id: "019f0000-0000-7000-8000-000000009e11"
                .to_owned()
                .try_into()?,
        });
        write(&fixture, offered.clone(), NOW.unix_millis()).await?;
        assert_eq!(read(&fixture).await?, offered);
        fixture.close().await?;
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Invalid ADR tags/classes/partial references and missing or gapped child rows must fail closed."
)]
async fn adr_rejects_corrupt_tags_classes_partial_references_and_file_gaps() -> Result<(), TestError>
{
    for sql in [
        "UPDATE fleet_workspace_adrs SET status='unknown'",
        "UPDATE fleet_workspace_adrs SET status=x'70726f706f736564'",
        "UPDATE fleet_workspace_adrs SET revision='01'",
        "UPDATE fleet_workspace_adrs SET revision='18446744073709551615'",
        "UPDATE fleet_workspace_adrs SET revision=x'31'",
        "UPDATE fleet_workspace_adrs SET made_in_kind='task'",
        "UPDATE fleet_workspace_adrs SET made_in_record_id='019f0000-0000-7000-8000-000000009e11'",
        "UPDATE fleet_workspace_adrs SET made_in_kind='unknown',made_in_record_id='019f0000-0000-7000-8000-000000009e11'",
        "UPDATE fleet_workspace_adrs SET repository_file_count=15",
        "UPDATE fleet_workspace_adrs SET repository_file_count=16.5",
        "UPDATE fleet_workspace_adrs SET actor_kind='unknown'",
        "DELETE FROM fleet_adr_repository_files WHERE ordinal=15",
        "DELETE FROM fleet_adr_repository_files WHERE ordinal=7; UPDATE fleet_workspace_adrs SET repository_file_count=15",
        "UPDATE fleet_adr_repository_files SET ordinal=7.5 WHERE ordinal=7",
        "UPDATE fleet_adr_repository_files SET commit_sha='bad' WHERE ordinal=0",
        "UPDATE fleet_adr_repository_files SET repository_origin='HTTPS://Example.test/repo' WHERE ordinal=0",
        "UPDATE fleet_adr_repository_files SET path=x'31' WHERE ordinal=0",
        "UPDATE fleet_adr_repository_files SET id='00000000-0000-4000-8000-000000000001' WHERE ordinal=0",
    ] {
        let fixture = seeded().await?;
        write(&fixture, adr(0)?, NOW.unix_millis()).await?;
        mutate(&fixture, sql).await?;
        assert!(
            matches!(read(&fixture).await, Err(StoreError::Corrupt { .. })),
            "invalid ADR accepted: {sql}"
        );
        fixture.close().await?;
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Literal approved ADR parent/child columns reject the unchanged JSON schema."
)]
async fn adr_and_repository_file_schemas_have_only_the_approved_typed_columns()
-> Result<(), TestError> {
    let fixture = prepare().await?;
    let proof = async {
        for (sql, expected) in [
            (
                "PRAGMA table_info(fleet_workspace_adrs)",
                vec![
                    ("adr_id", "TEXT", 1),
                    ("workspace_id", "TEXT", 0),
                    ("title", "TEXT", 0),
                    ("text", "TEXT", 0),
                    ("status", "TEXT", 0),
                    ("supersedes_adr_id", "TEXT", 0),
                    ("made_in_kind", "TEXT", 0),
                    ("made_in_record_id", "TEXT", 0),
                    ("revision", "TEXT", 0),
                    ("repository_file_count", "INTEGER", 0),
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
                ],
            ),
            (
                "PRAGMA table_info(fleet_adr_repository_files)",
                vec![
                    ("id", "TEXT", 1),
                    ("adr_id", "TEXT", 0),
                    ("ordinal", "INTEGER", 0),
                    ("repository_origin", "TEXT", 0),
                    ("path", "TEXT", 0),
                    ("commit_sha", "TEXT", 0),
                    ("created_at", "INTEGER", 0),
                    ("updated_at", "INTEGER", 0),
                ],
            ),
        ] {
            let mut actual: Vec<(String, String, i64)> = fixture
                .object
                .transaction(move |scope| {
                    Box::pin(async move {
                        query::<sqlx_turso::Turso>(sql)
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
            let mut expected = expected
                .into_iter()
                .map(|(name, kind, pk)| (name.to_owned(), kind.to_owned(), pk))
                .collect::<Vec<_>>();
            actual.sort_unstable();
            expected.sort_unstable();
            assert_eq!(
                actual, expected,
                "ADR storage must expose its exact typed shape: {sql}"
            );
        }
        Ok::<_, TestError>(())
    }
    .await;
    let closed = fixture.close().await;
    closed?;
    proof
}
