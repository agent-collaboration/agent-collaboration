//! Fleet import headers, phases and ordered manifest rows have typed physical storage.
use super::workspace_columns_tests::prepare;
use crate::fleet_storage_fixture::TestError;
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use sqlx_core::{query::query, row::Row};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The approved typed import header, phase and ordered manifest tables must replace header/phase JSON."
)]
async fn admin_import_schema_exposes_typed_header_phase_and_manifest_children()
-> Result<(), TestError> {
    let fixture = prepare().await?;
    for (table, expected) in [
        (
            "fleet_admin_imports",
            vec![
                "import_id",
                "project_id",
                "digest",
                "provenance_installation",
                "provenance_machine",
                "declared_owner",
                "manifest_epoch",
                "manifest_sequence",
                "schema_version",
                "schema_fingerprint",
                "pages",
                "manifest_table_count",
                "summary_kind",
                "summary_time_basis",
                "summary_record_at",
                "phase_kind",
                "published_epoch",
                "published_sequence",
                "created_at",
                "updated_at",
            ],
        ),
        (
            "fleet_admin_import_manifest_tables",
            vec![
                "row_id",
                "import_id",
                "ordinal",
                "table_name",
                "row_count",
                "table_digest",
                "created_at",
                "updated_at",
            ],
        ),
    ] {
        let columns = fixture
            .object
            .transaction(move |scope| {
                Box::pin(async move {
                    let sql = match table {
                        "fleet_admin_imports" => "PRAGMA table_info(fleet_admin_imports)",
                        "fleet_admin_import_manifest_tables" => {
                            "PRAGMA table_info(fleet_admin_import_manifest_tables)"
                        }
                        _ => return Err(StoreError::backend("unknown import fixture table")),
                    };
                    query::<sqlx_turso::Turso>(sql)
                        .fetch_all(scope.executor())
                        .await
                        .map_err(StoreError::backend)?
                        .into_iter()
                        .map(|row| {
                            Ok((
                                row.try_get::<String, _>("name")
                                    .map_err(StoreError::backend)?,
                                row.try_get::<String, _>("type")
                                    .map_err(StoreError::backend)?,
                                row.try_get::<i64, _>("pk").map_err(StoreError::backend)?,
                            ))
                        })
                        .collect::<Result<Vec<_>, StoreError>>()
                })
            })
            .await?;
        assert_eq!(columns.len(), expected.len(), "{table}: {columns:?}");
        for (index, name) in expected.into_iter().enumerate() {
            let storage = if [
                "pages",
                "manifest_table_count",
                "ordinal",
                "created_at",
                "updated_at",
            ]
            .contains(&name)
            {
                "INTEGER"
            } else {
                "TEXT"
            };
            assert!(
                columns.contains(&(name.into(), storage.into(), i64::from(index == 0))),
                "{table}.{name}: {columns:?}"
            );
        }
    }
    fixture.close().await?;
    Ok(())
}

type Fixture = crate::fleet_storage_fixture::FleetStorageFixture;
type Store =
    fleet_sync_tokio_runner::TokioObject<crate::CoreHost<crate::fleet_storage_fixture::FixtureIo>>;

pub(super) fn header(legacy: bool) -> Result<crate::AdminImportHeader, TestError> {
    use fleet_sync_protocol::{
        CommitSequence, DatabaseId, Epoch, ImportDigest, ReplicationPosition, SchemaIdentity,
        SnapshotManifest, TableDigest, TableManifest, TableName,
    };
    Ok(crate::AdminImportHeader {
        digest: ImportDigest::new("ab".repeat(32))?,
        provenance: fleet_admission::ImportProvenance {
            installation: "550e8400-e29b-41d4-a716-4466554400a9"
                .to_owned()
                .try_into()?,
            machine: "original import provenance λ".to_owned().try_into()?,
        },
        declared_owner: " literal declared owner ".to_owned().try_into()?,
        manifest: SnapshotManifest {
            database: DatabaseId::try_from("019f0000-0000-7000-8000-00000000de01".to_owned())?,
            position: ReplicationPosition::new(Epoch::new(1)?, CommitSequence::new(u64::MAX)),
            schema: SchemaIdentity::new(u64::MAX, "literal schema \0 fingerprint".into())?,
            tables: (0..16)
                .map(|ordinal| {
                    Ok(TableManifest {
                        table: TableName::new(if ordinal % 4 == 0 {
                            "z-last"
                        } else {
                            "a-first"
                        })?,
                        row_count: if ordinal == 0 {
                            u64::MAX
                        } else {
                            9_007_199_254_740_993
                        },
                        digest: TableDigest::new(if ordinal % 2 == 0 {
                            "cd".repeat(32)
                        } else {
                            "ef".repeat(32)
                        })?,
                    })
                })
                .collect::<Result<Vec<_>, TestError>>()?,
        },
        pages: u32::MAX,
        summary: board_sync_protocol::BoardAnnotation::ProjectSummary {
            latest_record_at: if legacy {
                message_board::RecordTime::LegacyUnknown
            } else {
                message_board::RecordTime::Router("2026-11-04T10:11:12.123456789Z".parse()?)
            },
        },
    })
}
pub(super) fn id(suffix: u16) -> Result<fleet_sync_protocol::RequestId, TestError> {
    Ok(format!("019f0000-0000-7000-8000-00000000{suffix:04x}").try_into()?)
}
pub(super) async fn command(
    fixture: &Fixture,
    command: crate::admin_import_store::AdminImportCommand,
    at: i64,
) -> Result<Result<crate::AdminImportRecord, crate::AdminImportRefusal>, StoreError> {
    fixture.object.transaction(move |scope|Box::pin(async move {
        let (record,_)=<crate::admin_import_store::AdminImportStep as crate::step_driver::StepCallback<Store,sqlx_turso::Turso>>::run(
            crate::admin_import_store::AdminImportStep {
                command,now:runtime_primitives::SyncTime::from_unix_millis(at),
                row_ids:crate::fleet_row_ids::FleetRowIds::new(std::sync::Arc::new(crate::fleet_storage_fixture::FixtureIo)),
            },scope
        ).await?;
        Ok(record)
    })).await
}
pub(super) async fn read(
    fixture: &Fixture,
    header: crate::AdminImportHeader,
) -> Result<Option<crate::AdminImportRecord>, StoreError> {
    fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                crate::admin_import_columns::read(scope, header.database(), &header.digest).await
            })
        })
        .await
}
pub(super) async fn physical(fixture: &Fixture) -> Result<(Vec<String>, Vec<String>), StoreError> {
    fixture.object.transaction(|scope|Box::pin(async move {
        let roots=query::<sqlx_turso::Turso>("SELECT json_array(import_id,project_id,digest,provenance_installation,provenance_machine,declared_owner,manifest_epoch,manifest_sequence,schema_version,schema_fingerprint,pages,manifest_table_count,summary_kind,summary_time_basis,summary_record_at,phase_kind,published_epoch,published_sequence,created_at,updated_at) AS state FROM fleet_admin_imports ORDER BY import_id")
            .fetch_all(scope.executor()).await.map_err(StoreError::backend)?.into_iter().map(|row|row.try_get("state").map_err(StoreError::backend)).collect::<Result<Vec<String>,_>>()?;
        let children=query::<sqlx_turso::Turso>("SELECT json_array(row_id,import_id,ordinal,table_name,row_count,table_digest,created_at,updated_at) AS state FROM fleet_admin_import_manifest_tables ORDER BY import_id,ordinal")
            .fetch_all(scope.executor()).await.map_err(StoreError::backend)?.into_iter().map(|row|row.try_get("state").map_err(StoreError::backend)).collect::<Result<Vec<String>,_>>()?;
        Ok((roots,children))
    })).await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Exact original headers, full-width counters, vector order/duplicates, all phases, injected timestamps and IDs must survive owner transitions, no-op replay and reopen."
)]
async fn admin_import_header_and_ordered_manifest_survive_phases_replay_and_owning_reopen()
-> Result<(), TestError> {
    use crate::fleet_storage_fixture::NOW;
    use crate::{
        AdminImportPhase, AdminImportRecord, AdminImportRefusal,
        admin_import_store::AdminImportCommand,
    };
    for legacy in [false, true] {
        let fixture = prepare().await?;
        let offered = header(legacy)?;
        let import_id = id(0xdf01)?;
        let reserved = AdminImportRecord {
            import_id,
            header: offered.clone(),
            phase: AdminImportPhase::Reserved,
        };
        assert_eq!(
            command(
                &fixture,
                AdminImportCommand::Reserve {
                    header: Box::new(offered.clone()),
                    candidate: import_id
                },
                NOW.unix_millis()
            )
            .await?,
            Ok(reserved.clone())
        );
        assert_eq!(
            read(&fixture, offered.clone()).await?,
            Some(reserved.clone())
        );
        let first = physical(&fixture).await?;
        assert_eq!(
            command(
                &fixture,
                AdminImportCommand::Read {
                    database: offered.database(),
                    digest: offered.digest.clone()
                },
                NOW.unix_millis() + 1
            )
            .await?,
            Ok(reserved.clone())
        );
        assert_eq!(
            command(
                &fixture,
                AdminImportCommand::Published {
                    database: offered.database(),
                    digest: offered.digest.clone(),
                    import_id: id(0xdf02)?,
                    position: offered.manifest.position
                },
                NOW.unix_millis() + 1
            )
            .await?,
            Err(AdminImportRefusal::IdentityMismatch)
        );
        let wrong_position = fleet_sync_protocol::ReplicationPosition::new(
            fleet_sync_protocol::Epoch::new(1)?,
            fleet_sync_protocol::CommitSequence::new(0),
        );
        assert_eq!(
            command(
                &fixture,
                AdminImportCommand::Published {
                    database: offered.database(),
                    digest: offered.digest.clone(),
                    import_id,
                    position: wrong_position
                },
                NOW.unix_millis() + 1
            )
            .await?,
            Err(AdminImportRefusal::PositionMismatch)
        );
        assert_eq!(physical(&fixture).await?, first);
        assert_eq!(first.0.len(), 1);
        assert_eq!(first.1.len(), 16);
        let original: serde_json::Value =
            serde_json::from_str(first.0.first().ok_or("missing root")?)?;
        assert_eq!(
            original.get(18),
            Some(&serde_json::json!(NOW.unix_millis()))
        );
        assert_eq!(
            original.get(19),
            Some(&serde_json::json!(NOW.unix_millis()))
        );
        for child in &first.1 {
            let child: Vec<serde_json::Value> = serde_json::from_str(child)?;
            let row_id = child
                .first()
                .and_then(serde_json::Value::as_str)
                .ok_or("missing child ID")?;
            assert!(runtime_primitives::uuid_v7(durable_steps::Uuid::parse_str(
                row_id
            )?));
            assert_eq!(child.get(6), Some(&serde_json::json!(NOW.unix_millis())));
            assert_eq!(child.get(7), Some(&serde_json::json!(NOW.unix_millis())));
        }
        assert_eq!(
            command(
                &fixture,
                AdminImportCommand::Complete {
                    database: offered.database(),
                    digest: offered.digest.clone(),
                    import_id
                },
                NOW.unix_millis() + 1
            )
            .await?,
            Err(AdminImportRefusal::NotPublished)
        );
        assert_eq!(
            command(
                &fixture,
                AdminImportCommand::Reserve {
                    header: Box::new(offered.clone()),
                    candidate: id(0xdf02)?
                },
                NOW.unix_millis() + 2
            )
            .await?,
            Ok(reserved)
        );
        assert_eq!(physical(&fixture).await?, first);
        let mut changed = offered.clone();
        changed.manifest.tables.reverse();
        assert!(matches!(
            command(
                &fixture,
                AdminImportCommand::Reserve {
                    header: Box::new(changed.clone()),
                    candidate: id(0xdf02)?
                },
                NOW.unix_millis() + 3
            )
            .await?,
            Err(AdminImportRefusal::ImportAttemptInProgress { .. })
        ));
        assert_eq!(physical(&fixture).await?, first);
        let published = AdminImportRecord {
            import_id,
            header: offered.clone(),
            phase: AdminImportPhase::Published {
                position: offered.manifest.position,
            },
        };
        assert_eq!(
            command(
                &fixture,
                AdminImportCommand::Published {
                    database: offered.database(),
                    digest: offered.digest.clone(),
                    import_id,
                    position: offered.manifest.position
                },
                NOW.unix_millis() + 17
            )
            .await?,
            Ok(published.clone())
        );
        let second = physical(&fixture).await?;
        assert_eq!(second.1, first.1);
        let root: serde_json::Value =
            serde_json::from_str(second.0.first().ok_or("missing published")?)?;
        assert_eq!(root.get(18), Some(&serde_json::json!(NOW.unix_millis())));
        assert_eq!(
            root.get(19),
            Some(&serde_json::json!(NOW.unix_millis() + 17))
        );
        assert_eq!(
            command(
                &fixture,
                AdminImportCommand::Published {
                    database: offered.database(),
                    digest: offered.digest.clone(),
                    import_id,
                    position: offered.manifest.position
                },
                NOW.unix_millis() + 18
            )
            .await?,
            Ok(published.clone())
        );
        assert_eq!(
            command(
                &fixture,
                AdminImportCommand::Reserve {
                    header: Box::new(changed),
                    candidate: id(0xdf03)?
                },
                NOW.unix_millis() + 19
            )
            .await?,
            Ok(published.clone())
        );
        assert_eq!(physical(&fixture).await?, second);
        let fixture = fixture.reopen().await?;
        assert_eq!(read(&fixture, offered.clone()).await?, Some(published));
        let complete = AdminImportRecord {
            import_id,
            header: offered.clone(),
            phase: AdminImportPhase::Complete {
                position: offered.manifest.position,
            },
        };
        assert_eq!(
            command(
                &fixture,
                AdminImportCommand::Complete {
                    database: offered.database(),
                    digest: offered.digest.clone(),
                    import_id
                },
                NOW.unix_millis() + 29
            )
            .await?,
            Ok(complete.clone())
        );
        let third = physical(&fixture).await?;
        assert_eq!(third.1, first.1);
        let root: serde_json::Value =
            serde_json::from_str(third.0.first().ok_or("missing complete")?)?;
        assert_eq!(root.get(18), Some(&serde_json::json!(NOW.unix_millis())));
        assert_eq!(
            root.get(19),
            Some(&serde_json::json!(NOW.unix_millis() + 29))
        );
        assert_eq!(
            command(
                &fixture,
                AdminImportCommand::Complete {
                    database: offered.database(),
                    digest: offered.digest.clone(),
                    import_id
                },
                NOW.unix_millis() + 30
            )
            .await?,
            Ok(complete.clone())
        );
        assert_eq!(physical(&fixture).await?, third);
        let fixture = fixture.reopen().await?;
        assert_eq!(read(&fixture, offered).await?, Some(complete));
        assert_eq!(physical(&fixture).await?, third);
        fixture.close().await?;
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A late real manifest primary-key conflict must roll back the complete header and every ordered child before owning reopen and retry."
)]
async fn admin_import_late_child_key_failure_rolls_back_header_and_manifest()
-> Result<(), TestError> {
    let fixture = prepare().await?;
    let offered = header(false)?;
    let original = offered.clone();
    let result=fixture.object.transaction(move |scope|Box::pin(async move {
        let mut ids=crate::fleet_row_ids::FleetRowIds::new(std::sync::Arc::new(crate::fleet_storage_fixture::FixtureIo));
        crate::admin_import_writes::insert(scope,&crate::AdminImportRecord {
            import_id:id(0xdf01).map_err(|error|StoreError::backend(error.to_string()))?,header:offered,phase:crate::AdminImportPhase::Reserved,
        },crate::fleet_storage_fixture::NOW,&mut ids).await?;
        query::<sqlx_turso::Turso>("INSERT INTO fleet_admin_import_manifest_tables SELECT * FROM fleet_admin_import_manifest_tables LIMIT 1")
            .execute(scope.executor()).await.map_err(StoreError::backend)?;
        Ok(())
    })).await;
    assert!(matches!(result, Err(StoreError::Backend { .. })));
    assert_eq!(physical(&fixture).await?, (Vec::new(), Vec::new()));
    let fixture = fixture.reopen().await?;
    assert_eq!(read(&fixture, original.clone()).await?, None);
    assert_eq!(physical(&fixture).await?, (Vec::new(), Vec::new()));
    command(
        &fixture,
        crate::admin_import_store::AdminImportCommand::Reserve {
            header: Box::new(original.clone()),
            candidate: id(0xdf01)?,
        },
        crate::fleet_storage_fixture::NOW.unix_millis() + 17,
    )
    .await?
    .map_err(|refusal| format!("unexpected import refusal: {refusal:?}"))?;
    assert_eq!(
        read(&fixture, original.clone())
            .await?
            .ok_or("missing retry")?
            .header,
        original
    );
    fixture.close().await?;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Invalid first headers write nothing; an empty manifest remains valid without synthetic rows, and a malformed stored primary key is refused."
)]
async fn admin_import_first_header_guards_and_empty_manifest_use_existing_domain_rules()
-> Result<(), TestError> {
    use crate::admin_import_store::AdminImportCommand;
    let fixture = prepare().await?;
    for invalid_epoch in [false, true] {
        let mut offered = header(false)?;
        if invalid_epoch {
            offered.manifest.position = fleet_sync_protocol::ReplicationPosition::new(
                fleet_sync_protocol::Epoch::new(2)?,
                fleet_sync_protocol::CommitSequence::new(1),
            );
        } else {
            offered.pages = 0;
        }
        assert_eq!(
            command(
                &fixture,
                AdminImportCommand::Reserve {
                    header: Box::new(offered),
                    candidate: id(0xdf01)?
                },
                crate::fleet_storage_fixture::NOW.unix_millis()
            )
            .await?,
            Err(crate::AdminImportRefusal::InvalidHeader)
        );
        assert_eq!(physical(&fixture).await?, (Vec::new(), Vec::new()));
    }
    let mut offered = header(true)?;
    offered.manifest.tables.clear();
    let record = command(
        &fixture,
        AdminImportCommand::Reserve {
            header: Box::new(offered.clone()),
            candidate: id(0xdf01)?,
        },
        crate::fleet_storage_fixture::NOW.unix_millis(),
    )
    .await?
    .map_err(|refusal| format!("reserve failed: {refusal:?}"))?;
    assert_eq!(read(&fixture, offered.clone()).await?, Some(record));
    assert!(physical(&fixture).await?.1.is_empty());
    fixture.object.transaction(|scope|Box::pin(async move {
        query::<sqlx_turso::Turso>("UPDATE fleet_admin_imports SET import_id='550e8400-e29b-41d4-a716-4466554400a9'")
            .execute(scope.executor()).await.map_err(StoreError::backend)?;
        Ok(())
    })).await?;
    assert!(matches!(
        read(&fixture, offered).await,
        Err(StoreError::Corrupt { .. })
    ));
    fixture.close().await?;
    Ok(())
}
