//! Feed bodies and ordered snapshot children replace stored JSON without mutable-record joins.
use super::workspace_columns_tests::prepare;
use crate::fleet_storage_fixture::TestError;
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use sqlx_core::{query::query, row::Row};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The approved feed and ordered snapshot child columns must be physical typed fields."
)]
async fn feed_schema_exposes_typed_snapshots_and_ordered_children() -> Result<(), TestError> {
    let fixture = prepare().await?;
    for (table, expected) in [
        (
            "fleet_feed",
            vec![
                "entry_id",
                "position",
                "record_kind",
                "record_id",
                "workspace_id",
                "board_id",
                "actor_kind",
                "actor_human_id",
                "actor_service_id",
                "actor_endpoint_id",
                "actor_session_id",
                "actor_router_component",
                "envelope_request_id",
                "router_time",
                "time_basis",
                "origin_installation",
                "origin_machine",
                "created_at",
                "updated_at",
                "as_of",
                "name",
                "description",
                "workspace_state",
                "repository_count",
                "board_kind",
                "initiative_goal",
                "initiative_done_when",
                "board_state",
                "project_title",
                "adr_title",
                "adr_text",
                "adr_status",
                "supersedes_adr_id",
                "made_in_kind",
                "made_in_record_id",
                "adr_revision",
                "repository_file_count",
                "ledger_request_id",
            ],
        ),
        (
            "fleet_feed_repository_refs",
            vec![
                "row_id",
                "entry_id",
                "ordinal",
                "reference_kind",
                "normalized_origin",
                "service_id",
                "common_directory",
                "created_at",
                "updated_at",
            ],
        ),
        (
            "fleet_feed_adr_repository_files",
            vec![
                "row_id",
                "entry_id",
                "ordinal",
                "repository_origin",
                "path",
                "commit_sha",
                "created_at",
                "updated_at",
            ],
        ),
    ] {
        let columns: Vec<(String, String, i64)> = fixture
            .object
            .transaction(move |scope| {
                Box::pin(async move {
                    let sql = match table {
                        "fleet_feed" => "PRAGMA table_info(fleet_feed)",
                        "fleet_feed_repository_refs" => {
                            "PRAGMA table_info(fleet_feed_repository_refs)"
                        }
                        "fleet_feed_adr_repository_files" => {
                            "PRAGMA table_info(fleet_feed_adr_repository_files)"
                        }
                        _ => return Err(StoreError::backend("unknown feed fixture table")),
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
            .await?;
        assert_eq!(columns.len(), expected.len(), "{table}: {columns:?}");
        for (index, name) in expected.into_iter().enumerate() {
            let storage = if [
                "position",
                "ordinal",
                "repository_count",
                "repository_file_count",
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

pub(super) fn records() -> Result<Vec<message_board::Record>, TestError> {
    use message_board::*;
    let mut workspace = super::workspace_columns_tests::workspace(1)?;
    let Record::Workspace(value) = &mut workspace else {
        return Err("wrong fixture kind".into());
    };
    let origin = RepositoryRef::Origin {
        normalized_origin: "example.test/history".to_owned().try_into()?,
    };
    let local = RepositoryRef::Local {
        service_id: "019f0000-0000-7000-8000-000000009f31"
            .to_owned()
            .try_into()?,
        common_directory: "/history/λ".to_owned().try_into()?,
    };
    value.repositories = (0..16)
        .map(|index| {
            if index % 4 == 0 {
                local.clone()
            } else {
                origin.clone()
            }
        })
        .collect();
    let mut envelope = value.envelope.clone();
    let workspace_id = value.workspace_id.clone();
    let board_id: BoardId = "019f0000-0000-7000-8000-000000009f32"
        .to_owned()
        .try_into()?;
    envelope.id = board_id.clone().into();
    envelope.scope.board = Some(board_id.clone());
    if let RecordPlacement::Fleet(placement) = &mut envelope.position {
        placement.position = FleetPosition::try_from(2)?;
    }
    let board = Record::Board(Board {
        envelope: envelope.clone(),
        board_id: board_id.clone(),
        workspace_id,
        name: "Historical board".to_owned().try_into()?,
        description: "original description".to_owned().try_into()?,
        kind: BoardKind::Initiative {
            goal: "literal goal".into(),
            done_when: "literal finish".into(),
        },
        state: BoardState::Closed,
    });
    let project_id: ProjectId = "019f0000-0000-7000-8000-000000009f33"
        .to_owned()
        .try_into()?;
    envelope.id = project_id.clone().into();
    if let RecordPlacement::Fleet(placement) = &mut envelope.position {
        placement.position = FleetPosition::try_from(3)?;
    }
    let project = Record::DirectoryEntry(ProjectDirectoryEntry {
        envelope: envelope.clone(),
        id: project_id,
        board: board_id,
        title: "Historical project".to_owned().try_into()?,
    });
    let adr_id: AdrId = "019f0000-0000-7000-8000-000000009f34"
        .to_owned()
        .try_into()?;
    envelope.id = adr_id.clone().into();
    envelope.scope.board = None;
    if let RecordPlacement::Fleet(placement) = &mut envelope.position {
        placement.position = FleetPosition::try_from(4)?;
    }
    let adr = Record::Adr(Adr {
        envelope: envelope.clone(),
        adr_id,
        workspace: envelope.scope.workspace.clone(),
        title: "Historical ADR".to_owned().try_into()?,
        text: "Literal **Markdown** λ".to_owned().try_into()?,
        status: AdrStatus::Accepted,
        supersedes: None,
        made_in: Some(RecordRef {
            kind: RecordKind::Thread,
            id: "019f0000-0000-7000-8000-000000009f35"
                .to_owned()
                .try_into()?,
        }),
        repository_files: (0..16)
            .map(|ordinal| {
                Ok(ArtifactRef {
                    repository: "example.test/history".to_owned().try_into()?,
                    path: format!("literal/{}/λ\0tail", 15 - ordinal),
                    commit: if ordinal % 2 == 0 {
                        Some("ab".repeat(20).try_into()?)
                    } else {
                        None
                    },
                })
            })
            .collect::<Result<Vec<_>, TestError>>()?,
        revision: SubjectRevision::try_from(i64::MAX as u64)?,
    });
    let mut offered = vec![workspace, board, project, adr];
    for (position, id, decision) in [
        (
            5,
            "019f0000-0000-7000-8000-000000009f36",
            StoredDecision::accepted(Vec::new(), ChangeOutcome::AlreadySo),
        ),
        (
            6,
            "019f0000-0000-7000-8000-000000009f37",
            StoredDecision::Rejected {
                reasons: vec![Refusal::ProjectRejected {
                    reason: Box::new(Refusal::InvalidShape {
                        field: "literal.field".into(),
                    }),
                }],
            },
        ),
    ] {
        let request_id: RequestId = id.to_owned().try_into()?;
        envelope.id = request_id.clone().into();
        envelope.time = RecordTime::Router("2026-11-04T10:11:12.123456789Z".parse()?);
        envelope.actor = RecordedActor::Session {
            session: SessionRef {
                endpoint: SessionEndpointRef {
                    service_id: "019f0000-0000-7000-8000-000000009f31"
                        .to_owned()
                        .try_into()?,
                    endpoint_id: "codex-local".to_owned().try_into()?,
                },
                session_id: "historical requester".to_owned().try_into()?,
            },
        };
        if let RecordPlacement::Fleet(placement) = &mut envelope.position {
            placement.position = FleetPosition::try_from(position)?;
        }
        offered.push(Record::Request(RequestLedgerEntry {
            envelope: envelope.clone(),
            request_id,
            canonical_hash: "ab".repeat(32).try_into()?,
            accepted_in: None,
            decision,
        }));
    }
    Ok(offered)
}

pub(super) async fn append(
    fixture: &crate::fleet_storage_fixture::FleetStorageFixture,
    records: Vec<message_board::Record>,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), StoreError> {
    fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                for record in &records {
                    if let message_board::Record::Request(entry) = record {
                        crate::fleet_decision_step::persist_decision(
                            scope,
                            crate::fleet_decision_step::FleetDecisionStep {
                                entry: entry.clone(),
                                effects: Vec::new(),
                                receipt_ids: Vec::new(),
                                #[cfg(feature = "conformance")]
                                fault: crate::fleet_decision_step::FleetDecisionFault::None,
                            },
                        )
                        .await?;
                    }
                }
                let entries =
                    records
                        .into_iter()
                        .map(|record| {
                            Ok(fleet_admission::FleetFeedEntry {
                                position: record.envelope().position.fleet_position().ok_or_else(
                                    || StoreError::backend("wrong fixture placement"),
                                )?,
                                record,
                            })
                        })
                        .collect::<Result<Vec<_>, StoreError>>()?;
                let mut ids = crate::fleet_row_ids::FleetRowIds::new(std::sync::Arc::new(
                    crate::fleet_storage_fixture::FixtureIo,
                ));
                crate::fleet_feed_store::append(scope, &entries, now, &mut ids).await
            })
        })
        .await
}
pub(super) async fn read(
    fixture: &crate::fleet_storage_fixture::FleetStorageFixture,
) -> Result<Vec<message_board::Record>, StoreError> {
    fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                let state = crate::fleet_feed_store::state(scope).await?;
                Ok(
                    crate::fleet_feed_store::read_range(scope, state.floor, state.head)
                        .await?
                        .into_iter()
                        .map(|entry| entry.record)
                        .collect(),
                )
            })
        })
        .await
}
pub(super) fn now() -> Result<chrono::DateTime<chrono::Utc>, TestError> {
    chrono::DateTime::from_timestamp_millis(crate::fleet_storage_fixture::NOW.unix_millis())
        .ok_or_else(|| "invalid fixture clock".into())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Historical bodies, exact ordered vectors and immutable ledger references must outlive canonical changes and owning reopen."
)]
async fn feed_all_five_kinds_keep_exact_snapshots_ordered_lists_and_request_envelopes_after_reopen()
-> Result<(), TestError> {
    let fixture = prepare().await?;
    let offered = records()?;
    append(&fixture, offered.clone(), now()?).await?;
    // No canonical records exist yet: a history read cannot depend on mutable resource rows.
    assert_eq!(read(&fixture).await?, offered);
    let mut current = offered[0].clone();
    let message_board::Record::Workspace(value) = &mut current else {
        return Err("wrong fixture kind".into());
    };
    value.name = "Later canonical name".to_owned().try_into()?;
    value.repositories.reverse();
    super::workspace_columns_tests::write(
        &fixture,
        current,
        crate::fleet_storage_fixture::NOW.unix_millis() + 17,
    )
    .await?;
    assert_eq!(read(&fixture).await?, offered);
    let times = fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                let rows = query::<sqlx_turso::Turso>(
                    "SELECT entry_id,created_at,updated_at FROM fleet_feed ORDER BY position",
                )
                .fetch_all(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                rows.into_iter()
                    .map(|row| {
                        Ok((
                            row.try_get::<String, _>("entry_id")
                                .map_err(StoreError::backend)?,
                            row.try_get::<i64, _>("created_at")
                                .map_err(StoreError::backend)?,
                            row.try_get::<i64, _>("updated_at")
                                .map_err(StoreError::backend)?,
                        ))
                    })
                    .collect::<Result<Vec<_>, StoreError>>()
            })
        })
        .await?;
    assert_eq!(times.len(), 6);
    for (id, created, updated) in &times {
        assert_eq!(durable_steps::Uuid::parse_str(id)?.get_version_num(), 7);
        assert_eq!(
            (*created, *updated),
            (
                crate::fleet_storage_fixture::NOW.unix_millis(),
                crate::fleet_storage_fixture::NOW.unix_millis()
            )
        );
    }
    let child_times=fixture.object.transaction(|scope|Box::pin(async move {
        let row=query::<sqlx_turso::Turso>("SELECT count(*) AS children,COALESCE(sum(CASE WHEN created_at=?1 AND updated_at=?1 THEN 0 ELSE 1 END),0) AS wrong FROM (SELECT created_at,updated_at FROM fleet_feed_repository_refs UNION ALL SELECT created_at,updated_at FROM fleet_feed_adr_repository_files)")
            .bind(crate::fleet_storage_fixture::NOW.unix_millis()).fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
        Ok((row.try_get::<i64,_>("children").map_err(StoreError::backend)?,row.try_get::<i64,_>("wrong").map_err(StoreError::backend)?))
    })).await?;
    assert_eq!(
        child_times,
        (32, 0),
        "snapshot children keep the first owner ingestion time after canonical changes"
    );
    let fixture = fixture.reopen().await?;
    assert_eq!(read(&fixture).await?, offered);
    let reopened = fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                query::<sqlx_turso::Turso>("SELECT entry_id FROM fleet_feed ORDER BY position")
                    .fetch_all(scope.executor())
                    .await
                    .map_err(StoreError::backend)?
                    .into_iter()
                    .map(|row| {
                        row.try_get::<String, _>("entry_id")
                            .map_err(StoreError::backend)
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
        })
        .await?;
    assert_eq!(
        reopened,
        times.into_iter().map(|row| row.0).collect::<Vec<_>>()
    );
    fixture.close().await?;
    Ok(())
}
