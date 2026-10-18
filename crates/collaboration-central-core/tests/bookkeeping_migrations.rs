#![allow(clippy::expect_used)]

#[allow(dead_code)]
mod support;

use chrono::{DateTime, Utc};
use collaboration_central_core::{
    BoundStatement, CallerKind, CentralError, CopyBookkeepingMetadata, CopyCall, CopyResult,
    IdBatch, ObjectAddress, ObjectCallId, RegistryTime, SqlRow, SqlTx, StoreError, WireCall,
    WireEffect, WireInput, WireResult, read_copy_bookkeeping_metadata,
    record_copy_migration_failure, step,
};
use support::SqlTxFake;

fn now() -> RegistryTime {
    RegistryTime(
        DateTime::parse_from_rfc3339("2026-10-16T10:00:00Z")
            .expect("time")
            .with_timezone(&Utc),
    )
}

fn has_table(store: &SqlTxFake, name: &str) -> bool {
    store
        .connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name=?)",
            [name],
            |row| row.get(0),
        )
        .expect("schema probe")
}

#[test]
fn tolerant_reader_distinguishes_absent_matching_damaged_and_recorded_metadata() {
    let mut store = SqlTxFake::blank();
    assert_eq!(
        read_copy_bookkeeping_metadata(&mut store).expect("absent ledger never prepares"),
        CopyBookkeepingMetadata::Uninitialized
    );
    store.connection.execute_batch("CREATE TABLE _copy_migration_failure(version INTEGER NOT NULL,reason TEXT NOT NULL,at TEXT NOT NULL)").expect("failure without first migration");
    assert_eq!(
        read_copy_bookkeeping_metadata(&mut store).expect("failure sidecar alone"),
        CopyBookkeepingMetadata::Uninitialized
    );
    store
        .connection
        .execute_batch(include_str!("fixtures/preversioned-copy-v1.sql"))
        .expect("old schema");
    assert_eq!(
        read_copy_bookkeeping_metadata(&mut store).expect("legacy metadata"),
        CopyBookkeepingMetadata::Unrecorded { shape_v1: true }
    );
    store.connection.execute_batch("CREATE TABLE IF NOT EXISTS _copy_directory_outbox(update_key TEXT PRIMARY KEY,call_id TEXT NOT NULL UNIQUE,record TEXT NOT NULL)").expect("legitimate optional old directory producer table");
    assert_eq!(
        read_copy_bookkeeping_metadata(&mut store).expect("valid old auxiliary shape"),
        CopyBookkeepingMetadata::Unrecorded { shape_v1: true }
    );
    store
        .connection
        .execute_batch("ALTER TABLE _copy_receipts ADD COLUMN unexpected TEXT")
        .expect("damage legacy shape");
    assert_eq!(
        read_copy_bookkeeping_metadata(&mut store).expect("damaged legacy metadata"),
        CopyBookkeepingMetadata::Unrecorded { shape_v1: false }
    );
    store.connection.execute_batch("CREATE TABLE _copy_migrations(version INTEGER PRIMARY KEY,applied_at TEXT NOT NULL,from_version INTEGER NOT NULL); INSERT INTO _copy_migrations VALUES(1,'2026-10-16T10:00:00Z',1)").expect("recorded metadata");
    assert_eq!(
        read_copy_bookkeeping_metadata(&mut store).expect("recorded metadata"),
        CopyBookkeepingMetadata::Recorded { version: 1 }
    );
    store
        .connection
        .execute_batch("INSERT INTO _copy_migrations VALUES(2,'2026-10-16T10:01:00Z',1)")
        .expect("later version");
    assert_eq!(
        read_copy_bookkeeping_metadata(&mut store).expect("latest version"),
        CopyBookkeepingMetadata::Recorded { version: 2 }
    );
}

#[test]
fn tolerant_reader_rejects_a_partial_or_semantically_changed_legacy_shape() {
    for damage in [
        "DROP TABLE _copy_import_pages",
        "DROP TABLE _copy_receipts; CREATE TABLE _copy_receipts(epoch TEXT,from_epoch TEXT NOT NULL,from_commit TEXT NOT NULL,through_epoch TEXT NOT NULL,through_commit TEXT NOT NULL,digest TEXT NOT NULL,PRIMARY KEY(epoch,from_epoch,from_commit,through_epoch,through_commit))",
        "DROP TABLE _copy_import_pages; CREATE TABLE _copy_import_pages(page_index INTEGER, record TEXT NOT NULL)",
        "CREATE TABLE _copy_unknown(value TEXT)",
        "CREATE TABLE _copy_directory_outbox(update_key TEXT PRIMARY KEY,record TEXT NOT NULL)",
    ] {
        let mut store = SqlTxFake::blank();
        store
            .connection
            .execute_batch(include_str!("fixtures/preversioned-copy-v1.sql"))
            .expect("old schema");
        store
            .connection
            .execute_batch(damage)
            .expect("literal shape damage");
        assert_eq!(
            read_copy_bookkeeping_metadata(&mut store).expect("tolerant damaged metadata"),
            CopyBookkeepingMetadata::Unrecorded { shape_v1: false },
            "{damage}"
        );
        assert!(!has_table(&store, "_copy_migrations"));
    }
}

#[test]
fn recorded_metadata_costs_a_schema_probe_and_one_version_read_without_a_cache() {
    struct ReadCounter<'a> {
        store: &'a mut SqlTxFake,
        reads: usize,
    }
    impl SqlTx for ReadCounter<'_> {
        fn execute(&mut self, statement: BoundStatement) -> Result<u64, StoreError> {
            self.store.execute(statement)
        }
        fn query(&mut self, statement: BoundStatement) -> Result<Vec<SqlRow>, StoreError> {
            self.reads += 1;
            self.store.query(statement)
        }
    }
    let mut store = SqlTxFake::blank();
    {
        let mut counter = ReadCounter {
            store: &mut store,
            reads: 0,
        };
        assert_eq!(
            read_copy_bookkeeping_metadata(&mut counter).expect("empty"),
            CopyBookkeepingMetadata::Uninitialized
        );
        assert_eq!(counter.reads, 1);
    }
    store.connection.execute_batch("CREATE TABLE _copy_migrations(version INTEGER PRIMARY KEY,applied_at TEXT NOT NULL,from_version INTEGER NOT NULL); INSERT INTO _copy_migrations VALUES(1,'2026-10-16T10:00:00Z',0)").expect("recorded");
    for _ in 0..2 {
        let mut counter = ReadCounter {
            store: &mut store,
            reads: 0,
        };
        assert_eq!(
            read_copy_bookkeeping_metadata(&mut counter).expect("recorded"),
            CopyBookkeepingMetadata::Recorded { version: 1 }
        );
        assert_eq!(counter.reads, 2);
    }
}

#[test]
fn upgrading_recorded_v1_does_not_stamp_a_malformed_existing_outbox_as_current() {
    let mut store = SqlTxFake::blank();
    store
        .connection
        .execute_batch(include_str!("fixtures/preversioned-copy-v1.sql"))
        .expect("legacy schema");
    store.connection.execute_batch("CREATE TABLE _copy_migrations(version INTEGER PRIMARY KEY,applied_at TEXT NOT NULL,from_version INTEGER NOT NULL); INSERT INTO _copy_migrations VALUES(1,'2026-10-15T10:00:00Z',1); CREATE TABLE _copy_directory_outbox(update_key TEXT PRIMARY KEY,record TEXT NOT NULL)").expect("recorded v1 with malformed old outbox");
    let result = store.run(
        WireInput::Alarm {
            face: CallerKind::Copy,
        },
        now(),
    );
    assert!(
        matches!(result,Err(CentralError::CopyMigrationPending { failure,.. }) if failure.version==2),
        "malformed bookkeeping must fail the migration before current is stamped"
    );
    assert_eq!(
        read_copy_bookkeeping_metadata(&mut store).expect("old ledger preserved"),
        CopyBookkeepingMetadata::Recorded { version: 1 }
    );
}

#[test]
fn tolerant_reader_refuses_empty_or_invalid_recorded_versions() {
    for rows in [
        "",
        "INSERT INTO _copy_migrations VALUES(0)",
        "INSERT INTO _copy_migrations VALUES(-1)",
        "INSERT INTO _copy_migrations VALUES(1.5)",
        "INSERT INTO _copy_migrations VALUES('broken')",
    ] {
        let mut store = SqlTxFake::blank();
        store
            .connection
            .execute_batch("CREATE TABLE _copy_migrations(version)")
            .expect("invalid ledger fixture");
        store
            .connection
            .execute_batch(rows)
            .expect("literal invalid version");
        assert!(
            read_copy_bookkeeping_metadata(&mut store).is_err(),
            "invalid ledger must not become Uninitialized or version zero: {rows}"
        );
    }
}

#[test]
fn late_migration_sql_failure_rolls_back_its_write_and_records_diagnostics_separately() {
    let mut store = SqlTxFake::blank();
    store
        .connection
        .execute_batch(include_str!("fixtures/preversioned-copy-v1.sql"))
        .expect("legacy schema");
    store.connection.execute_batch(
        "CREATE TABLE records(id INTEGER PRIMARY KEY,label TEXT NOT NULL);
         INSERT INTO records VALUES(7,'keep');
         CREATE TABLE _copy_migrations(version INTEGER PRIMARY KEY,applied_at TEXT NOT NULL,from_version INTEGER NOT NULL);
         INSERT INTO _copy_migrations VALUES(1,'2026-10-15T10:00:00Z',1);
         CREATE TRIGGER reject_copy_v2 BEFORE INSERT ON _copy_migrations WHEN NEW.version=2 BEGIN SELECT RAISE(ABORT,'injected migration version failure'); END;"
    ).expect("actual SQL failure after the next DDL write");
    assert!(!has_table(&store, "_copy_migration_failure"));
    assert!(!has_table(&store, "_copy_directory_outbox"));
    let call_id = ObjectCallId::new("019f0000-0000-7000-8000-000000000003").expect("call");
    let input = WireInput::Call {
        from: ObjectAddress::Registry,
        call_id: call_id.clone(),
        call: Box::new(WireCall::Copy(CopyCall::CopyStatus {
            project: collaboration_replication_protocol::ProjectId::new(
                "019f0000-0000-7000-8000-000000000001",
            )
            .expect("project"),
        })),
    };
    store
        .connection
        .execute_batch("BEGIN IMMEDIATE")
        .expect("event begins");
    let error = step(&mut store, input.clone(), now(), IdBatch::default())
        .expect_err("version stamp actually fails");
    assert!(
        has_table(&store, "_copy_directory_outbox"),
        "migration performed a real DDL write before failing"
    );
    let failure = match &error {
        CentralError::CopyMigrationPending { failure, .. } => Some(failure),
        _ => None,
    }
    .expect("typed migration failure");
    assert_eq!(failure.version, 2);
    assert!(!failure.is_full());
    assert!(
        failure
            .reason()
            .contains("injected migration version failure")
    );
    store
        .connection
        .execute_batch("ROLLBACK")
        .expect("event rolls back");
    assert!(!has_table(&store, "_copy_directory_outbox"));
    assert!(!has_table(&store, "_copy_migration_failure"));
    store
        .connection
        .execute_batch("BEGIN IMMEDIATE")
        .expect("separate diagnostic transaction");
    record_copy_migration_failure(&mut store, failure, now()).expect("create and record failure");
    store
        .connection
        .execute_batch("COMMIT")
        .expect("diagnostic commit");
    let recorded: (i64, String, String) = store
        .connection
        .query_row(
            "SELECT version,reason,at FROM _copy_migration_failure",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("durable failure");
    assert_eq!(recorded, (2, failure.reason(), now().0.to_rfc3339()));
    assert_eq!(
        read_copy_bookkeeping_metadata(&mut store).expect("still old version"),
        CopyBookkeepingMetadata::Recorded { version: 1 }
    );
    assert_eq!(
        error.rollback_trace(),
        vec![WireEffect::Return {
            call_id,
            result: WireResult::Copy(CopyResult::Unavailable(
                collaboration_replication_protocol::CopyUnavailableReason::MigrationPending {
                    kind: collaboration_replication_protocol::BookkeepingKind::Copy,
                    version: 2
                }
            ))
        }]
    );
    store
        .connection
        .execute_batch("DROP TRIGGER reject_copy_v2")
        .expect("remove scoped failure");
    let effects = store.run(input, now()).expect("same call after repair");
    assert!(matches!(
        effects.as_slice(),
        [WireEffect::Return {
            result: WireResult::Copy(CopyResult::CopyStatus(None)),
            ..
        }]
    ));
    assert_eq!(
        read_copy_bookkeeping_metadata(&mut store).expect("current after repair"),
        CopyBookkeepingMetadata::Recorded { version: 2 }
    );
    assert_eq!(
        store
            .connection
            .query_row("SELECT COUNT(*) FROM _copy_migration_failure", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("cleared diagnostic"),
        0
    );
    assert_eq!(
        store
            .connection
            .query_row("SELECT label FROM records WHERE id=7", [], |row| row
                .get::<_, String>(0))
            .expect("preserved project row"),
        "keep"
    );
}

#[test]
fn full_during_the_first_migration_preserves_its_type_and_can_record_after_rollback() {
    struct OneWrite<'a> {
        store: &'a mut SqlTxFake,
        writes: usize,
    }
    impl SqlTx for OneWrite<'_> {
        fn execute(&mut self, statement: BoundStatement) -> Result<u64, StoreError> {
            if self.writes == 1 {
                return Err(StoreError::Full);
            }
            self.writes += 1;
            self.store.execute(statement)
        }
        fn query(&mut self, statement: BoundStatement) -> Result<Vec<SqlRow>, StoreError> {
            self.store.query(statement)
        }
    }
    let mut store = SqlTxFake::blank();
    let call_id = ObjectCallId::new("019f0000-0000-7000-8000-000000000003").expect("call");
    let input = WireInput::Call {
        from: ObjectAddress::Registry,
        call_id: call_id.clone(),
        call: Box::new(WireCall::Copy(CopyCall::CopyStatus {
            project: collaboration_replication_protocol::ProjectId::new(
                "019f0000-0000-7000-8000-000000000001",
            )
            .expect("project"),
        })),
    };
    store
        .connection
        .execute_batch("BEGIN IMMEDIATE")
        .expect("event begins");
    let error = step(
        &mut OneWrite {
            store: &mut store,
            writes: 0,
        },
        input.clone(),
        now(),
        IdBatch::default(),
    )
    .expect_err("second migration write hits capacity");
    assert!(has_table(&store, "_copy_state"));
    let failure = match &error {
        CentralError::CopyMigrationPending { failure, .. } => Some(failure),
        _ => None,
    }
    .expect("migration failure");
    assert_eq!(failure.version, 1);
    assert!(
        failure.is_full(),
        "do not flatten StoreError::Full into an opaque string"
    );
    store
        .connection
        .execute_batch("ROLLBACK")
        .expect("event rollback");
    assert!(!has_table(&store, "_copy_state"));
    assert!(!has_table(&store, "_copy_migrations"));
    assert!(!has_table(&store, "_copy_migration_failure"));
    store
        .connection
        .execute_batch("BEGIN IMMEDIATE")
        .expect("independent failure transaction");
    record_copy_migration_failure(&mut store, failure, now())
        .expect("record even though first migration failed");
    store
        .connection
        .execute_batch("COMMIT")
        .expect("failure commit");
    assert_eq!(
        read_copy_bookkeeping_metadata(&mut store).expect("only failure sidecar"),
        CopyBookkeepingMetadata::Uninitialized
    );
    assert_eq!(
        error.rollback_trace(),
        vec![WireEffect::Return {
            call_id,
            result: WireResult::Copy(CopyResult::Unavailable(
                collaboration_replication_protocol::CopyUnavailableReason::StorageFull
            ))
        }]
    );
    collaboration_central_core::validate_call_returns(&input, &error.rollback_trace())
        .expect("one matching typed answer");
    assert_eq!(
        store
            .connection
            .query_row("SELECT version FROM _copy_migration_failure", [], |row| row
                .get::<_, i64>(0))
            .expect("failure persisted"),
        1
    );
}

#[test]
fn first_copy_event_versions_an_uninitialized_store() {
    let mut store = SqlTxFake::blank();
    assert!(!has_table(&store, "_copy_state"));
    assert!(!has_table(&store, "_copy_migrations"));

    store
        .run(
            WireInput::Alarm {
                face: CallerKind::Copy,
            },
            now(),
        )
        .expect("first copy event");

    assert!(
        has_table(&store, "_copy_migrations"),
        "first event must record the bookkeeping migration"
    );
    let first: (i64, i64) = store
        .connection
        .query_row(
            "SELECT version,from_version FROM _copy_migrations ORDER BY version LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("first migration");
    assert_eq!(first, (1, 0));
    assert!(has_table(&store, "_copy_state"));
}

#[test]
fn populated_preversioned_copy_is_adopted_without_changing_project_rows_or_position() {
    let mut store = SqlTxFake::blank();
    store
        .connection
        .execute_batch(include_str!("fixtures/preversioned-copy-v1.sql"))
        .expect("accepted preversioned schema");
    store.connection.execute_batch(
        "CREATE TABLE records(id INTEGER PRIMARY KEY, label TEXT NOT NULL);
         INSERT INTO records VALUES(7,'preserved legacy row');
         INSERT INTO _copy_state VALUES(1,'019f0000-0000-7000-8000-000000000001','019f0000-0000-7000-8000-000000000002','1','1','0','1','3','1','aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',0);
         INSERT INTO _copy_summary VALUES(1,'open','2026-10-15T09:00:00+00:00','1','3','2026-10-15T10:00:00+00:00');
         INSERT INTO _copy_receipts VALUES('1','1','0','1','3','bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb');
         INSERT INTO _copy_schema_steps VALUES('1',0,'CREATE TABLE records(id INTEGER PRIMARY KEY, label TEXT NOT NULL)');"
    ).expect("populated old copy");
    // This existing protocol helper builds a valid fixture input. Migration
    // behavior is checked against the unchanged input, not a migration helper.
    let fingerprint = collaboration_replication_protocol::schema_fingerprint(&[
        collaboration_replication_protocol::SchemaEntry {
            kind: collaboration_replication_protocol::SchemaObjectKind::Table,
            name: "records".into(),
            sql: "CREATE TABLE records(id INTEGER PRIMARY KEY, label TEXT NOT NULL)".into(),
        },
    ])
    .expect("fixture project schema identity");
    store
        .connection
        .execute(
            "UPDATE _copy_state SET schema_fingerprint=?",
            [&fingerprint],
        )
        .expect("valid old fingerprint");
    let old_state = "SELECT json_array(singleton,project,holder,fence_epoch,start_epoch,start_commit,acked_epoch,acked_commit,schema_version,schema_fingerprint,archived) FROM _copy_state";
    let old_summary = "SELECT json_array(singleton,status,latest_record_at,position_epoch,position_commit,acked_at) FROM _copy_summary";
    let before_state: String = store
        .connection
        .query_row(old_state, [], |row| row.get(0))
        .expect("old state");
    let before_summary: String = store
        .connection
        .query_row(old_summary, [], |row| row.get(0))
        .expect("old summary");
    assert!(
        !has_table(&store, "_copy_migrations"),
        "fixture must really predate versioning"
    );

    store
        .run(
            WireInput::Alarm {
                face: CallerKind::Copy,
            },
            now(),
        )
        .expect("legacy copy event");

    assert!(
        has_table(&store, "_copy_migrations"),
        "legacy shape must be adopted"
    );
    let adopted: (i64, i64) = store
        .connection
        .query_row(
            "SELECT version,from_version FROM _copy_migrations WHERE version=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("adoption record");
    assert_eq!(adopted, (1, 1));
    let record: (i64, String) = store
        .connection
        .query_row("SELECT id,label FROM records", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .expect("original row");
    assert_eq!(record, (7, "preserved legacy row".into()));
    let position: (String, String, String, String) = store.connection.query_row(
        "SELECT fence_epoch,acked_epoch,acked_commit,schema_fingerprint FROM _copy_state WHERE singleton=1", [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    ).expect("original position");
    assert_eq!(position, ("1".into(), "1".into(), "3".into(), fingerprint));
    assert_eq!(
        store
            .connection
            .query_row(old_state, [], |row| row.get::<_, String>(0))
            .expect("same state"),
        before_state
    );
    assert_eq!(
        store
            .connection
            .query_row(old_summary, [], |row| row.get::<_, String>(0))
            .expect("same summary"),
        before_summary
    );
    assert_eq!(
        store
            .connection
            .query_row("SELECT statement FROM _copy_schema_steps", [], |row| row
                .get::<_, String>(
                0
            ))
            .expect("same project schema step"),
        "CREATE TABLE records(id INTEGER PRIMARY KEY, label TEXT NOT NULL)"
    );
    assert_eq!(
        store
            .connection
            .query_row("SELECT last_piece_at FROM _copy_state", [], |row| row
                .get::<_, String>(0))
            .expect("backfilled actual retained timestamp"),
        "2026-10-15T10:00:00+00:00"
    );
}
