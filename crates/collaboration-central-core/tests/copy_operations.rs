#![allow(clippy::expect_used)]

#[allow(dead_code)]
mod support;

use chrono::{DateTime, TimeDelta, Utc};
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
use support::SqlTxFake;

fn now() -> RegistryTime {
    RegistryTime(
        DateTime::parse_from_rfc3339("2026-10-16T10:00:00Z")
            .expect("time")
            .with_timezone(&Utc),
    )
}
fn project() -> ProjectId {
    ProjectId::new("019f0000-0000-7000-8000-000000000001").expect("project")
}
fn machine() -> InstallationId {
    InstallationId::new("019f0000-0000-7000-8000-000000000002").expect("machine")
}
fn call_id() -> ObjectCallId {
    ObjectCallId::new("019f0000-0000-7000-8000-000000000003").expect("call")
}
fn position(commit: u64) -> ReplicationPosition {
    ReplicationPosition::new(Epoch::new(1).expect("epoch"), commit.into())
}
fn call_copy(store: &mut SqlTxFake, call: CopyCall, time: RegistryTime) -> CopyResult {
    let input = WireInput::Call {
        from: ObjectAddress::Registry,
        call_id: call_id(),
        call: Box::new(WireCall::Copy(call)),
    };
    let effects = store.run(input.clone(), time).expect("copy event");
    validate_call_returns(&input, &effects).expect("one matching Return");
    effects
        .into_iter()
        .find_map(|effect| match effect {
            WireEffect::Return {
                result: WireResult::Copy(result),
                ..
            } => Some(result),
            _ => None,
        })
        .expect("copy Return")
}
fn health(store: &mut SqlTxFake, time: RegistryTime) -> LiveCopyHealth {
    store
        .connection
        .execute_batch("BEGIN IMMEDIATE")
        .expect("Health transaction");
    let effects = step_copy(
        store,
        CopyInput::Health {
            call_id: call_id(),
            storage_bytes: 8_589_934_592,
        },
        time,
        IdBatch::default(),
    )
    .expect("measured Health");
    store
        .connection
        .execute_batch("COMMIT")
        .expect("Health commit");
    effects
        .into_iter()
        .find_map(|effect| match effect {
            CopyEffect::Return {
                result: CopyResult::Health(health),
                ..
            } => match *health {
                CopyHealth::Live(report) => Some(*report),
                CopyHealth::Retired { .. } => None,
            },
            _ => None,
        })
        .expect("Health Return")
}

#[test]
fn health_is_unit_transport_and_reads_old_metadata_without_running_the_prelude() {
    assert_eq!(
        serde_json::to_value(CopyOpsCall::Health {}).expect("unit request"),
        serde_json::json!({"kind":"health"})
    );
    for packet in [
        serde_json::json!({"kind":"health","storageBytes":7}),
        serde_json::json!({"kind":"health","value":{"storageBytes":7}}),
        serde_json::json!({"kind":"unknown"}),
    ] {
        assert!(serde_json::from_value::<CopyOpsCall>(packet).is_err());
    }
    let mut store = SqlTxFake::blank();
    store
        .connection
        .execute_batch(include_str!("fixtures/preversioned-copy-v1.sql"))
        .expect("old schema");
    store.connection.execute_batch("CREATE TABLE _copy_migrations(version INTEGER PRIMARY KEY,applied_at TEXT NOT NULL,from_version INTEGER NOT NULL); INSERT INTO _copy_migrations VALUES(1,'2026-10-15T10:00:00Z',1)").expect("old ledger");
    store.connection.execute_batch("INSERT INTO _copy_summary VALUES(1,'open','2026-10-15T09:00:00+00:00','1','3','2026-10-15T10:00:00+00:00'); INSERT INTO _copy_receipts VALUES('1','1','0','1','3','bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb')").expect("retained legacy piece timestamp");
    let before = store.connection.total_changes();
    let result = health(&mut store, now());
    assert_eq!(
        result.bookkeeping,
        CopyHealthRead::Known(CopyBookkeepingMetadata::Recorded { version: 1 })
    );
    assert_eq!(result.storage_bytes, 8_589_934_592);
    assert_eq!(
        result.last_piece_at,
        CopyHealthRead::Known(Some(
            DateTime::parse_from_rfc3339("2026-10-15T10:00:00Z")
                .expect("literal retained time")
                .with_timezone(&Utc)
        )),
        "Health must expose known legacy data before a failing migration can run"
    );
    assert_eq!(
        store.connection.total_changes(),
        before,
        "Health must not migrate or write diagnostics"
    );
    assert_eq!(
        store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE name='_copy_directory_outbox'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .expect("outbox remains absent"),
        0
    );
    let input = WireInput::Call {
        from: ObjectAddress::Registry,
        call_id: call_id(),
        call: Box::new(WireCall::Copy(CopyCall::Ops(CopyOpsCall::Health {}))),
    };
    assert!(
        store.run(input, now()).is_err(),
        "the core never invents a measurement for a raw wire input"
    );
}

#[test]
fn health_reports_the_last_new_piece_and_staged_parts_without_advancing_on_replay() {
    let mut store = SqlTxFake::new();
    assert!(matches!(
        call_copy(
            &mut store,
            CopyCall::Initialize {
                project: project(),
                epoch: position(0).epoch,
                holder: machine()
            },
            now()
        ),
        CopyResult::Initialized(_)
    ));
    assert!(
        matches!(health(&mut store,now()).copy,CopyHealthRead::Known(Some(CopyStatus { schema,.. })) if schema.fingerprint=="4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945"),
        "private migration tables never enter the project schema identity"
    );
    assert_eq!(
        health(&mut store, now()).last_piece_at,
        CopyHealthRead::Known(None)
    );
    let summary = || {
        Some(ProjectSummary {
            status: "open".into(),
            latest_record_at: now().0,
        })
    };
    let schema = Piece::new(PieceContents {
        project: project(),
        epoch: position(0).epoch,
        continues_from: position(0),
        through: position(1),
        transactions: vec![ReplicatedTransaction::SchemaStep {
            commit_id: 1.into(),
            migration_version: 1,
            statements: vec![
                "CREATE TABLE records(id INTEGER PRIMARY KEY,label TEXT NOT NULL)".into(),
            ],
        }],
        part: None,
        summary: summary(),
    })
    .expect("schema piece");
    assert_eq!(
        call_copy(
            &mut store,
            CopyCall::ApplyPiece {
                piece: schema.clone(),
                sender: machine()
            },
            now()
        ),
        CopyResult::ApplyPiece(SessionAnswer::Acked {
            through: position(1)
        })
    );
    assert_eq!(
        health(&mut store, now()).last_piece_at,
        CopyHealthRead::Known(Some(now().0))
    );
    let row = |id| {
        RowChange::upsert(
            "records",
            vec![SqlValue::Integer(id)],
            vec![SqlValue::Integer(id), SqlValue::Text("kept".into())],
        )
        .expect("row")
    };
    let transaction = ReplicatedTransaction::Rows {
        commit_id: 2.into(),
        rows: vec![row(1), row(2)],
    };
    let digest = PieceDigest::of_transaction(&transaction).expect("whole transaction digest");
    for index in 0..2 {
        let time = RegistryTime(now().0 + TimeDelta::seconds(10 * (i64::from(index) + 1)));
        let piece = Piece::new(PieceContents {
            project: project(),
            epoch: position(0).epoch,
            continues_from: position(1),
            through: position(2),
            transactions: vec![ReplicatedTransaction::Rows {
                commit_id: 2.into(),
                rows: vec![row(i64::from(index) + 1)],
            }],
            part: Some(
                TransactionPart::new(
                    TransactionKey {
                        epoch: position(0).epoch,
                        commit_id: 2.into(),
                    },
                    index,
                    2,
                    digest.clone(),
                )
                .expect("part"),
            ),
            summary: if index == 1 { summary() } else { None },
        })
        .expect("part piece");
        let answer = call_copy(
            &mut store,
            CopyCall::ApplyPiece {
                piece,
                sender: machine(),
            },
            time,
        );
        let report = health(&mut store, time);
        assert_eq!(report.last_piece_at, CopyHealthRead::Known(Some(time.0)));
        if index == 0 {
            assert!(matches!(
                answer,
                CopyResult::ApplyPiece(SessionAnswer::PartStaged { .. })
            ));
            assert_eq!(
                report.staged_parts,
                CopyHealthRead::Known(vec![StagedPartHealth {
                    transaction: TransactionKey {
                        epoch: position(0).epoch,
                        commit_id: 2.into()
                    },
                    index: 0,
                    staged_at: time.0
                }])
            );
        } else {
            assert_eq!(
                answer,
                CopyResult::ApplyPiece(SessionAnswer::Acked {
                    through: position(2)
                })
            );
            assert_eq!(report.staged_parts, CopyHealthRead::Known(Vec::new()));
        }
    }
    assert_eq!(
        call_copy(
            &mut store,
            CopyCall::ApplyPiece {
                piece: schema,
                sender: machine()
            },
            RegistryTime(now().0 + TimeDelta::seconds(30))
        ),
        CopyResult::ApplyPiece(SessionAnswer::Acked {
            through: position(1)
        })
    );
    let report = health(&mut store, now());
    assert_eq!(
        report.last_piece_at,
        CopyHealthRead::Known(Some(now().0 + TimeDelta::seconds(20)))
    );
    assert!(
        matches!(report.copy,CopyHealthRead::Known(Some(CopyStatus { acked,.. })) if acked==position(2))
    );
    assert_eq!(store.record_ids(), vec![1, 2]);
}

#[test]
fn legacy_health_uses_receipts_and_staged_times_without_treating_import_as_a_piece() {
    let mut store = SqlTxFake::blank();
    store
        .connection
        .execute_batch(include_str!("fixtures/preversioned-copy-v1.sql"))
        .expect("legacy schema");
    store.connection.execute_batch("INSERT INTO _copy_summary VALUES(1,'open','2026-10-15T09:00:00+00:00','1','3','2026-10-15T10:00:00+00:00')").expect("import-like summary without any piece receipt");
    assert_eq!(
        health(&mut store, now()).last_piece_at,
        CopyHealthRead::Known(None)
    );
    store.connection.execute_batch("INSERT INTO _copy_receipts VALUES('1','1','0','1','3','bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb')").expect("completed piece receipt");
    let expected = DateTime::parse_from_rfc3339("2026-10-15T10:00:00Z")
        .expect("literal time")
        .with_timezone(&Utc);
    assert_eq!(
        health(&mut store, now()).last_piece_at,
        CopyHealthRead::Known(Some(expected))
    );
    // Health reads staged metadata, not a decoded/re-applied piece payload.
    store
        .connection
        .execute_batch(
            "INSERT INTO _copy_staged_parts VALUES('1','4',0,'2026-10-15T06:30:00-04:00','{}')",
        )
        .expect("retained staged timestamp");
    let expected = DateTime::parse_from_rfc3339("2026-10-15T10:30:00Z")
        .expect("literal normalized time")
        .with_timezone(&Utc);
    assert_eq!(
        health(&mut store, now()).last_piece_at,
        CopyHealthRead::Known(Some(expected))
    );
    store
        .connection
        .execute_batch("UPDATE _copy_staged_parts SET staged_at='invalid'")
        .expect("corrupt timestamp");
    assert!(matches!(
        health(&mut store, now()).last_piece_at,
        CopyHealthRead::Unreadable { .. }
    ));
    assert_eq!(
        store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE name='_copy_migrations'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .expect("no Health migration"),
        0
    );
}
