//! The approved workspace schema keeps domain fields and ordered repositories typed.
use super::*;
use crate::fleet_storage_fixture::{FixtureIo, FleetStorageFixture, NOW, TestError};
use fleet_tokio_objects::TokioObject;
use message_board::{
    ActorOrigin, FleetFeedEntry, FleetPosition, Record, RecordEnvelope, RecordPlacement,
    RecordScope, RecordTime, RecordedActor, RepositoryRef, Workspace, WorkspaceState,
};
use sqlx_core::{query::query, row::Row};
use std::sync::Arc;

type Store = TokioObject<crate::CoreHost<FixtureIo>>;
type TestResult<T = ()> = Result<T, TestError>;

pub(crate) fn workspace(position: u64) -> TestResult<Record> {
    let id: message_board::WorkspaceId = "019f0000-0000-7000-8000-000000009e01"
        .to_owned()
        .try_into()?;
    let origin = RepositoryRef::Origin {
        normalized_origin: "example.test/z/project".to_owned().try_into()?,
    };
    Ok(Record::Workspace(Workspace {
        envelope: RecordEnvelope {
            id: id.clone().into(),
            scope: RecordScope {
                workspace: id.clone(),
                board: None,
                project: None,
            },
            actor: RecordedActor::Human {
                human_id: "owner@example.test".to_owned().try_into()?,
            },
            request: Some(
                "019f0000-0000-7000-8000-000000009e02"
                    .to_owned()
                    .try_into()?,
            ),
            position: RecordPlacement::Fleet(FleetFeedEntry {
                position: FleetPosition::try_from(position)?,
                origin: ActorOrigin {
                    installation: "019f0000-0000-7000-8000-000000009e03"
                        .to_owned()
                        .try_into()?,
                    machine: "workspace fixture".to_owned().try_into()?,
                },
            }),
            time: RecordTime::LegacyUnknown,
        },
        workspace_id: id,
        name: "typed-workspace".to_owned().try_into()?,
        description: "original description\nλ".to_owned().try_into()?,
        state: WorkspaceState::Active,
        repositories: vec![
            origin.clone(),
            RepositoryRef::Local {
                service_id: "019f0000-0000-7000-8000-000000009e04"
                    .to_owned()
                    .try_into()?,
                common_directory: "/workspace/repository".to_owned().try_into()?,
            },
            origin,
        ],
    }))
}

pub(crate) async fn write(
    fixture: &FleetStorageFixture,
    record: Record,
    millis: i64,
) -> TestResult {
    fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                let digest = fleet_admission::FleetImportDigest::new("ab".repeat(32))
                    .map_err(StoreError::backend)?;
                let mut ids = crate::fleet_row_ids::FleetRowIds::new(Arc::new(FixtureIo));
                crate::fleet_record_store::put(
                    scope,
                    &record,
                    crate::fleet_record_store::RecordWriteSource::Imported(&digest),
                    chrono::DateTime::from_timestamp_millis(millis)
                        .ok_or_else(|| StoreError::backend("invalid fixture clock"))?,
                    &mut ids,
                )
                .await
            })
        })
        .await?;
    Ok(())
}

async fn read(fixture: &FleetStorageFixture) -> Result<Record, StoreError> {
    fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                let reference: message_board::RecordRef = message_board::WorkspaceId::try_from(
                    "019f0000-0000-7000-8000-000000009e01".to_owned(),
                )
                .map_err(StoreError::backend)?
                .into();
                crate::fleet_record_store::read(scope, &reference)
                    .await?
                    .map(|record| record.record)
                    .ok_or_else(|| StoreError::backend("workspace absent"))
            })
        })
        .await
}

pub(super) async fn mutate(fixture: &FleetStorageFixture, sql: &'static str) -> TestResult {
    fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                sqlx_core::raw_sql::raw_sql(sql)
                    .execute(scope.executor())
                    .await
                    .map_err(StoreError::backend)?;
                Ok(())
            })
        })
        .await?;
    Ok(())
}

pub(crate) async fn prepare() -> TestResult<FleetStorageFixture> {
    let fixture = FleetStorageFixture::open().await?;
    fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                <BootstrapStep as StepCallback<Store, sqlx_turso::Turso>>::run(
                    BootstrapStep {
                        schema: EngineSchema::Fleet,
                        now: NOW,
                        clock: Arc::new(FixtureIo),
                    },
                    scope,
                )
                .await?;
                // Step 0 also creates the feed state row, as Fleet open does.
                crate::fleet_feed_store::ensure_state(
                    scope,
                    durable_steps::Uuid::parse_str("019f0000-0000-7000-8000-00000000f0f0")
                        .map_err(StoreError::backend)?,
                    chrono::DateTime::from_timestamp_millis(NOW.unix_millis())
                        .ok_or_else(|| StoreError::backend("fixture clock out of range"))?,
                )
                .await?;
                Ok(())
            })
        })
        .await?;
    Ok(fixture)
}

async fn columns(
    fixture: &FleetStorageFixture,
    table: &'static str,
) -> TestResult<Vec<(String, String, i64)>> {
    Ok(fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                let sql = match table {
                    "fleet_workspaces" => "PRAGMA table_info(fleet_workspaces)",
                    "fleet_workspace_repository_refs" => {
                        "PRAGMA table_info(fleet_workspace_repository_refs)"
                    }
                    _ => return Err(StoreError::backend("unexpected workspace table")),
                };
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
        .await?)
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The literal approved column inventory must reject JSON storage; setup errors propagate."
)]
async fn workspace_and_ordered_repository_schema_have_only_the_approved_typed_columns() -> TestResult
{
    let fixture = prepare().await?;
    let proof = async {
        let mut workspace = columns(&fixture, "fleet_workspaces").await?;
        let mut expected = [
            ("workspace_id", "TEXT", 1),
            ("name", "TEXT", 0),
            ("description", "TEXT", 0),
            ("state", "TEXT", 0),
            ("repository_count", "INTEGER", 0),
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
        .map(|(name, storage, pk)| (name.to_owned(), storage.to_owned(), pk))
        .collect::<Vec<_>>();
        workspace.sort_unstable();
        expected.sort_unstable();
        assert_eq!(
            workspace, expected,
            "canonical Workspace must contain its typed fields instead of a JSON payload"
        );
        let mut children = columns(&fixture, "fleet_workspace_repository_refs").await?;
        let mut expected = [
            ("row_id", "TEXT", 1),
            ("workspace_id", "TEXT", 0),
            ("ordinal", "INTEGER", 0),
            ("reference_kind", "TEXT", 0),
            ("normalized_origin", "TEXT", 0),
            ("service_id", "TEXT", 0),
            ("common_directory", "TEXT", 0),
            ("created_at", "INTEGER", 0),
            ("updated_at", "INTEGER", 0),
        ]
        .into_iter()
        .map(|(name, storage, pk)| (name.to_owned(), storage.to_owned(), pk))
        .collect::<Vec<_>>();
        children.sort_unstable();
        expected.sort_unstable();
        assert_eq!(
            children, expected,
            "the ordered list is distinct from the unique attachment index"
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
    reason = "The literal record, repository order and injected timestamps are storage contracts."
)]
async fn workspace_preserves_bounded_position_extremes_repository_order_and_creation_across_reopen()
-> TestResult {
    for position in [0, i64::MAX as u64] {
        let fixture = prepare().await?;
        let offered = workspace(position)?;
        write(&fixture, offered.clone(), NOW.unix_millis()).await?;
        assert_eq!(read(&fixture).await?, offered);
        let fixture = fixture.reopen().await?;
        assert_eq!(read(&fixture).await?, offered);
        let mut renamed = offered;
        let Record::Workspace(value) = &mut renamed else {
            return Err("wrong fixture kind".into());
        };
        value.name = "renamed-workspace".to_owned().try_into()?;
        write(&fixture, renamed.clone(), NOW.unix_millis() + 17).await?;
        assert_eq!(read(&fixture).await?, renamed);
        let times = fixture.object.transaction(|scope| Box::pin(async move {
            let row = query::<sqlx_turso::Turso>("SELECT created_at,updated_at,(SELECT count(*) FROM fleet_workspace_repository_refs) AS refs,(SELECT count(*) FROM fleet_repository_attachments) AS associations FROM fleet_workspaces").fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
            Ok((row.try_get::<i64,_>("created_at").map_err(StoreError::backend)?, row.try_get::<i64,_>("updated_at").map_err(StoreError::backend)?, row.try_get::<i64,_>("refs").map_err(StoreError::backend)?, row.try_get::<i64,_>("associations").map_err(StoreError::backend)?))
        })).await?;
        assert_eq!(times, (NOW.unix_millis(), NOW.unix_millis() + 17, 3, 2));
        fixture.close().await?;
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Corrupt typed values must fail the real owning-store decoder."
)]
async fn workspace_rejects_unknown_tags_wrong_sqlite_classes_and_repository_child_gaps()
-> TestResult {
    for sql in [
        "UPDATE fleet_workspaces SET state='unknown'",
        "UPDATE fleet_workspaces SET state=x'616374697665'",
        "UPDATE fleet_workspaces SET name=' padded '",
        "UPDATE fleet_workspaces SET actor_kind='unknown'",
        "UPDATE fleet_workspaces SET actor_endpoint_id='codex-local'",
        "UPDATE fleet_workspaces SET repository_count=3.5",
        "UPDATE fleet_workspaces SET fleet_position=1.5",
        "UPDATE fleet_workspaces SET created_at=x'3132'",
        "UPDATE fleet_workspace_repository_refs SET reference_kind='unknown' WHERE ordinal=0",
        "UPDATE fleet_workspace_repository_refs SET ordinal=0.5 WHERE ordinal=0",
        "UPDATE fleet_workspace_repository_refs SET service_id='019f0000-0000-7000-8000-000000009e04' WHERE ordinal=0",
        "DELETE FROM fleet_workspace_repository_refs WHERE ordinal=1",
        "DELETE FROM fleet_workspace_repository_refs WHERE ordinal=2",
        "DELETE FROM fleet_workspace_repository_refs WHERE ordinal=1; UPDATE fleet_workspaces SET repository_count=2",
        "UPDATE fleet_workspaces SET repository_count=2",
        "UPDATE fleet_workspace_repository_refs SET ordinal=-1 WHERE ordinal=0",
        "UPDATE fleet_workspace_repository_refs SET row_id='00000000-0000-4000-8000-000000000001' WHERE ordinal=0",
    ] {
        let fixture = prepare().await?;
        write(&fixture, workspace(1)?, NOW.unix_millis()).await?;
        mutate(&fixture, sql).await?;
        assert!(
            matches!(read(&fixture).await, Err(StoreError::Corrupt { .. })),
            "invalid workspace accepted: {sql}"
        );
        fixture.close().await?;
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "All declared actor variants and precise domain time must survive typed storage unchanged."
)]
async fn workspace_preserves_session_and_router_actor_variants_and_nanosecond_time() -> TestResult {
    let actors: Vec<RecordedActor> = vec![
        RecordedActor::Session {
            session: message_board::SessionRef {
                endpoint: message_board::SessionEndpointRef {
                    service_id: "019f0000-0000-7000-8000-000000009e05"
                        .to_owned()
                        .try_into()?,
                    endpoint_id: "codex-local".to_owned().try_into()?,
                },
                session_id: "opaque session identity".to_owned().try_into()?,
            },
        },
        RecordedActor::Router {
            component: message_board::RouterComponent::SubscriptionDelivery,
            installation: "019f0000-0000-7000-8000-000000009e05"
                .to_owned()
                .try_into()?,
        },
        RecordedActor::Router {
            component: message_board::RouterComponent::ArchiveObservation,
            installation: "019f0000-0000-7000-8000-000000009e05"
                .to_owned()
                .try_into()?,
        },
    ];
    for actor in actors {
        let fixture = prepare().await?;
        let mut offered = workspace(7)?;
        let Record::Workspace(value) = &mut offered else {
            return Err("wrong fixture kind".into());
        };
        value.envelope.actor = actor;
        value.envelope.request = None;
        value.envelope.time = RecordTime::Router("2026-11-04T10:11:12.123456789Z".parse()?);
        value.state = WorkspaceState::Archiving;
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
    reason = "The declared valid maximum resource text includes NUL bytes and must be preserved exactly."
)]
async fn workspace_preserves_embedded_nul_resource_text_at_the_declared_bounds() -> TestResult {
    let fixture = prepare().await?;
    let mut offered = workspace(1)?;
    let Record::Workspace(value) = &mut offered else {
        return Err("wrong fixture kind".into());
    };
    value.name = "\0".repeat(message_board::MAX_NAME_BYTES).try_into()?;
    value.description = "\0"
        .repeat(message_board::MAX_DESCRIPTION_BYTES)
        .try_into()?;
    write(&fixture, offered.clone(), NOW.unix_millis()).await?;
    assert_eq!(read(&fixture).await?, offered);
    fixture.close().await?;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Sixteen supplied child ordinals must stay numeric across the checked text projection."
)]
async fn workspace_repository_ordinals_remain_numeric_through_the_full_sixteen_entry_list()
-> TestResult {
    let fixture = prepare().await?;
    let mut offered = workspace(1)?;
    let Record::Workspace(value) = &mut offered else {
        return Err("wrong fixture kind".into());
    };
    value.repositories = (0..16)
        .map(|ordinal| {
            Ok(RepositoryRef::Origin {
                normalized_origin: format!("example.test/repository-{ordinal}").try_into()?,
            })
        })
        .collect::<TestResult<Vec<_>>>()?;
    write(&fixture, offered.clone(), NOW.unix_millis()).await?;
    assert_eq!(read(&fixture).await?, offered);
    fixture.close().await?;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A real late SQL error must roll back the parent and every ordered child row."
)]
async fn late_workspace_child_write_failure_keeps_original_parent_and_repository_list() -> TestResult
{
    let fixture = prepare().await?;
    let original = workspace(1)?;
    write(&fixture, original.clone(), NOW.unix_millis()).await?;
    mutate(&fixture, "CREATE TABLE workspace_late_fault (row_id TEXT NOT NULL PRIMARY KEY, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL); INSERT INTO workspace_late_fault VALUES ('019f0000-0000-7000-8000-000000009e07',0,0)").await?;
    let failure = fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                let mut changed =
                    workspace(2).map_err(|error| StoreError::backend(error.to_string()))?;
                let Record::Workspace(value) = &mut changed else {
                    return Err(StoreError::backend("wrong fixture kind"));
                };
                value.name = "must-rollback"
                    .to_owned()
                    .try_into()
                    .map_err(StoreError::backend)?;
                value.repositories.reverse();
                value.repositories.pop();
                let digest = fleet_admission::FleetImportDigest::new("ab".repeat(32))
                    .map_err(StoreError::backend)?;
                let mut ids = crate::fleet_row_ids::FleetRowIds::new(Arc::new(FixtureIo));
                crate::fleet_record_store::put(
                    scope,
                    &changed,
                    crate::fleet_record_store::RecordWriteSource::Imported(&digest),
                    chrono::DateTime::from_timestamp_millis(NOW.unix_millis() + 17)
                        .ok_or_else(|| StoreError::backend("invalid fixture clock"))?,
                    &mut ids,
                )
                .await?;
        query::<sqlx_turso::Turso>("INSERT INTO workspace_late_fault VALUES ('019f0000-0000-7000-8000-000000009e07',0,0)")
                    .execute(scope.executor())
                    .await
                    .map_err(StoreError::backend)?;
                Ok(())
            })
        })
        .await;
    assert!(
        failure.is_err(),
        "the real uniqueness fault must reject the transaction"
    );
    assert_eq!(read(&fixture).await?, original);
    let fixture = fixture.reopen().await?;
    assert_eq!(read(&fixture).await?, original);
    fixture.close().await?;
    Ok(())
}
