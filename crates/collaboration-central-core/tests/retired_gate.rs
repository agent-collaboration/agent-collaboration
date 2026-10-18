#![allow(clippy::expect_used)]
#[allow(dead_code)]
mod support;

use collaboration_central_core::*;
use collaboration_replication_protocol::*;
use serde_json::json;
use support::SqlTxFake;

const PROJECT: &str = "019f0000-0000-7000-8000-000000000001";
const TRANSFER: &str = "019f0000-0000-7000-8000-000000000002";

fn now() -> RegistryTime {
    RegistryTime(
        chrono::DateTime::parse_from_rfc3339("2026-10-16T12:00:00Z")
            .expect("later central time")
            .with_timezone(&chrono::Utc),
    )
}
fn call_id() -> ObjectCallId {
    ObjectCallId::new("019f0000-0000-7000-8000-000000000003").expect("call")
}
fn retired_store() -> SqlTxFake {
    let store = SqlTxFake::blank();
    store.connection.execute_batch(
        "CREATE TABLE _copy_state(singleton INTEGER PRIMARY KEY,retired INTEGER NOT NULL CHECK(retired IN(0,1)),retire_receipt TEXT);
         CREATE TABLE _copy_migrations(version INTEGER PRIMARY KEY,applied_at TEXT NOT NULL,from_version INTEGER NOT NULL);
         INSERT INTO _copy_migrations VALUES(999,'2026-10-16T10:00:00Z',998);"
    ).expect("minimal post-wipe state with an unsupported later migration");
    let receipt = json!({"transferId":TRANSFER,"fence":2,"acked":{"epoch":1,"commitId":7},"retiredAt":"2026-10-16T10:00:00Z"});
    store
        .connection
        .execute(
            "INSERT INTO _copy_state VALUES(1,1,?)",
            [receipt.to_string()],
        )
        .expect("receipt");
    store
}
fn call(store: &mut SqlTxFake, call: CopyCall) -> CopyResult {
    let input = WireInput::Call {
        from: ObjectAddress::Registry,
        call_id: call_id(),
        call: Box::new(WireCall::Copy(call)),
    };
    let effects = store
        .run(input.clone(), now())
        .expect("retirement precedes migration");
    validate_call_returns(&input, &effects).expect("matching Return");
    assert_eq!(effects.len(), 1, "no dispatch or alarm after retirement");
    effects
        .into_iter()
        .find_map(|effect| match effect {
            WireEffect::Return {
                result: WireResult::Copy(result),
                ..
            } => Some(result),
            _ => None,
        })
        .expect("copy result")
}

#[test]
fn retired_marker_precedes_even_a_newer_ledger_and_operations_do_no_work() {
    let mut store = retired_store();
    let changes = store.connection.total_changes();
    for call_body in [
        CopyCall::CopyStatus {
            project: ProjectId::new(PROJECT).expect("project"),
        },
        CopyCall::Ops(CopyOpsCall::Repair {}),
    ] {
        assert_eq!(
            serde_json::to_value(call(&mut store, call_body)).expect("answer"),
            json!({"kind":"retired"})
        );
    }
    assert_eq!(
        serde_json::to_value(call(&mut store, CopyCall::Ops(CopyOpsCall::Migrate {})))
            .expect("migration item"),
        json!({"kind":"migrated","value":{"kind":"retired"}})
    );
    assert!(
        store
            .run(
                WireInput::Alarm {
                    face: CallerKind::Copy
                },
                now()
            )
            .expect("stale alarm")
            .is_empty()
    );
    assert_eq!(store.connection.total_changes(), changes);
    let version: u64 = store
        .connection
        .query_row("SELECT max(version) FROM _copy_migrations", [], |row| {
            row.get(0)
        })
        .expect("ledger");
    assert_eq!(version, 999);
}

#[test]
fn retired_health_is_readable_from_the_minimal_receipt_without_other_bookkeeping() {
    let mut store = retired_store();
    store
        .connection
        .execute_batch("DROP TABLE _copy_migrations")
        .expect("wipe removed ledger");
    let changes = store.connection.total_changes();
    let effects = step_copy(
        &mut store,
        CopyInput::Health {
            call_id: call_id(),
            storage_bytes: 1,
        },
        now(),
        IdBatch::default(),
    )
    .expect("retired Health");
    let health = effects
        .into_iter()
        .find_map(|effect| match effect {
            CopyEffect::Return {
                result: CopyResult::Health(body),
                ..
            } => Some(body),
            _ => None,
        })
        .expect("Health result");
    assert_eq!(
        serde_json::to_value(health).expect("Health wire"),
        json!({
            "kind":"retired","value":{"transfer":TRANSFER,"retiredAt":"2026-10-16T10:00:00Z"}
        })
    );
    assert_eq!(store.connection.total_changes(), changes);
}

#[test]
fn health_reports_a_damaged_retire_receipt_instead_of_failing_the_whole_diagnostic() {
    let mut store = retired_store();
    store
        .connection
        .execute("UPDATE _copy_state SET retire_receipt='{}'", [])
        .expect("damaged receipt");
    let effects = step_copy(
        &mut store,
        CopyInput::Health {
            call_id: call_id(),
            storage_bytes: 1,
        },
        now(),
        IdBatch::default(),
    )
    .expect("Health still diagnoses damaged metadata");
    let health = effects
        .into_iter()
        .find_map(|effect| match effect {
            CopyEffect::Return {
                result: CopyResult::Health(body),
                ..
            } => Some(body),
            _ => None,
        })
        .expect("Health");
    assert!(
        matches!(*health, CopyHealth::Live(report) if matches!(report.copy, CopyHealthRead::Unreadable { .. }))
    );
}

#[test]
fn retired_data_calls_keep_their_typed_piece_read_and_import_results() {
    let mut store = retired_store();
    let project = ProjectId::new(PROJECT).expect("project");
    let epoch = Epoch::new(1).expect("epoch");
    let position = ReplicationPosition::new(epoch, 0.into());
    let summary = ProjectSummary {
        status: "imported".into(),
        latest_record_at: now().0,
    };
    let schema =
        SchemaIdentity::new(0, schema_fingerprint(&[]).expect("fingerprint")).expect("schema");
    let header = ImportSnapshotHeader {
        project: project.clone(),
        head: position,
        schema: schema.clone(),
        schema_steps: vec![],
    };
    let upload = ProjectImportPage {
        digest: ImportDigestBuilder::new(&header)
            .expect("header")
            .finish(&summary)
            .expect("digest"),
        index: 99, // Even an out-of-order offer must see the tombstone first.
        page: SnapshotPage {
            project: project.clone(),
            position,
            schema,
            schema_steps: vec![],
            table: String::new(),
            rows: vec![],
            next: None,
            as_of: now().0,
        },
        final_summary: Some(summary),
    };
    assert_eq!(
        call(
            &mut store,
            CopyCall::ImportSeed {
                project: project.clone(),
                upload: Box::new(upload)
            }
        ),
        CopyResult::ImportSeed(ImportSeedResult {
            answer: ProjectImportAnswer::Retired,
            unheld: None
        })
    );
    assert_eq!(
        call(&mut store, CopyCall::SnapshotPage { cursor: None }),
        CopyResult::SnapshotPage(SessionAnswer::ReadPage(ReadPage::ProjectCopy(
            ProjectCopyRead::Retired
        )))
    );
    let piece = Piece::new(PieceContents {
        project,
        epoch,
        continues_from: position,
        through: ReplicationPosition::new(epoch, 1.into()),
        transactions: vec![ReplicatedTransaction::SchemaStep {
            commit_id: 1.into(),
            migration_version: 1,
            statements: vec!["CREATE TABLE records(id INTEGER PRIMARY KEY)".into()],
        }],
        part: None,
        summary: Some(ProjectSummary {
            status: "active".into(),
            latest_record_at: now().0,
        }),
    })
    .expect("piece");
    assert_eq!(
        call(
            &mut store,
            CopyCall::ApplyPiece {
                piece,
                sender: InstallationId::new("019f0000-0000-7000-8000-000000000009")
                    .expect("sender"),
            }
        ),
        CopyResult::ApplyPiece(SessionAnswer::Refused(SessionRefusal::Retired))
    );
}
