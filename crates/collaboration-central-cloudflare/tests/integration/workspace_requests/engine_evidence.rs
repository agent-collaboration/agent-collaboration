//! Receipts the verification asks the workerd engine tests to emit (acceptance
//! R2/R3 visit order, R4 attempt dumps, V11.2 attempt IDs). Each receipt is
//! written under `$CENTRAL_ENGINE_EVIDENCE_DIR/workerd/` only when that variable
//! names a directory; unset, the tests assert the same facts and write nothing.
use super::{Statement, support::Fixture, workerd_host::RemoteRuntime};
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, ReadOutcome, RowExpectation, SqlValue,
    StatementBatch,
};
use std::io::Write;

pub(super) const EVIDENCE_DIR_VARIABLE: &str = "CENTRAL_ENGINE_EVIDENCE_DIR";

/// Appends `body` to the receipt file `name`, creating the host folder.
pub(super) fn append_receipt(name: &str, body: &str) {
    let Some(root) = std::env::var_os(EVIDENCE_DIR_VARIABLE) else {
        return;
    };
    let folder = std::path::Path::new(&root).join("workerd");
    std::fs::create_dir_all(&folder).unwrap();
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(folder.join(name))
        .unwrap();
    writeln!(file, "{body}").unwrap();
}

async fn read(runtime: &RemoteRuntime, sql: &'static str) -> ReadOutcome {
    runtime.read(Statement { sql, args: vec![] }).await.unwrap()
}

async fn run(runtime: &RemoteRuntime, sql: &'static str) {
    assert!(matches!(
        runtime
            .run_batch(StatementBatch {
                statements: vec![GuardedStatement {
                    statement: Statement { sql, args: vec![] },
                    expect: RowExpectation::Any,
                }],
                alarm: AlarmUpdate::Unchanged,
            })
            .await
            .unwrap(),
        BatchOutcome::Applied { .. }
    ));
}

/// Writes every column of every `central_work_attempts` row after one step
/// (acceptance R4), as one JSON line naming the step.
pub(super) async fn dump_attempts(runtime: &RemoteRuntime, receipt: &str, step: &str) {
    let table = read(
        runtime,
        "SELECT * FROM central_work_attempts ORDER BY machine_kind,subject_key",
    )
    .await;
    append_receipt(
        receipt,
        &serde_json::json!({ "step": step, "central_work_attempts": table }).to_string(),
    );
}

/// Records every attempt ID the store writes from now on, so a test can parse
/// each one as UUIDv7 (V11.2), including IDs a later success deletes. The
/// witness is a disposable test-store fixture: its counter key keeps write
/// order and it never reaches a production schema, so it carries no UUIDv7 key.
pub(super) async fn install_attempt_id_witness(runtime: &RemoteRuntime) {
    run(
        runtime,
        "CREATE TABLE IF NOT EXISTS test_attempt_id_witness (witness_order INTEGER PRIMARY KEY, attempt_id TEXT, write_kind TEXT NOT NULL)",
    )
    .await;
    run(
        runtime,
        "CREATE TRIGGER IF NOT EXISTS test_attempt_id_witness_insert AFTER INSERT ON central_work_attempts BEGIN INSERT INTO test_attempt_id_witness(attempt_id,write_kind) VALUES (NEW.attempt_id,'insert'); END",
    )
    .await;
    run(
        runtime,
        "CREATE TRIGGER IF NOT EXISTS test_attempt_id_witness_update AFTER UPDATE ON central_work_attempts BEGIN INSERT INTO test_attempt_id_witness(attempt_id,write_kind) VALUES (NEW.attempt_id,'update'); END",
    )
    .await;
}

/// Parses every witnessed attempt ID as UUIDv7, skipping only the IDs the test
/// itself planted, and writes them to the V11.2 receipt. Fails loudly when the
/// witness was never installed. Returns how many IDs were checked.
pub(super) async fn assert_every_written_attempt_id_is_uuid_v7(
    runtime: &RemoteRuntime,
    test: &str,
    planted: &[&str],
) -> usize {
    let witnessed = read(
        runtime,
        "SELECT attempt_id,write_kind FROM test_attempt_id_witness ORDER BY witness_order",
    )
    .await;
    let mut checked = Vec::new();
    for row in &witnessed.rows {
        let [SqlValue::Text(attempt_id), SqlValue::Text(write_kind)] = row.as_slice() else {
            panic!("{test}: a written attempt_id is not text: {row:?}")
        };
        if planted.contains(&attempt_id.as_str()) {
            continue;
        }
        let parsed = attempt_id
            .parse::<uuid::Uuid>()
            .unwrap_or_else(|error| panic!("{test}: attempt_id {attempt_id}: {error}"));
        assert_eq!(
            parsed.get_version_num(),
            7,
            "{test}: attempt_id {attempt_id} is not UUIDv7"
        );
        checked.push(serde_json::json!({ "attemptId": attempt_id, "write": write_kind }));
    }
    append_receipt(
        "v11-attempt-ids.jsonl",
        &serde_json::json!({ "test": test, "uuidV7": checked }).to_string(),
    );
    checked.len()
}

/// The fixture organization's central store, for the two witness steps.
pub(super) async fn witness_fixture_attempt_ids(fixture: &Fixture) {
    let runtime = fixture.host.central(&fixture.caller.organization_id).await;
    install_attempt_id_witness(&runtime).await;
}
pub(super) async fn assert_fixture_attempt_ids_are_uuid_v7(
    fixture: &Fixture,
    test: &str,
    planted: &[&str],
) -> usize {
    let runtime = fixture.host.central(&fixture.caller.organization_id).await;
    assert_every_written_attempt_id_is_uuid_v7(&runtime, test, planted).await
}
