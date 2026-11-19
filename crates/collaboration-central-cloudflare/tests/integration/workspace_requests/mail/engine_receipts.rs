//! V7 and V11 on the workerd host: typed probe receipts through the real
//! request_log, and the live schema of the one failure table.
use super::super::{Statement, support::*};
use super::engine_probe_support::*;
use central_storage::{CorruptReason, SqlValue, StoreFailure};
use collaboration_central_core::engine_probe::{
    EngineReceipt, EngineRefusal, EngineRequestOutcome, ProbeEvent, ProbeRefusal, ProbeState,
};

/// Every row a request could write: items, the group counter, receipts, records.
async fn row_counts(runtime: &Runtime) -> Vec<Vec<SqlValue>> {
    runtime.statement_host().read(Statement {
        sql: "SELECT (SELECT COUNT(*) FROM engine_probe_items),(SELECT group_concat(state) FROM (SELECT state FROM engine_probe_items ORDER BY item_id)),(SELECT SUM(effect_count) FROM engine_probe_groups),(SELECT COUNT(*) FROM runtime_request_log WHERE request_id IN (?,?,?,?)),(SELECT COUNT(*) FROM engine_work_attempts)",
        args: [9410, 9411, 9412, 9422]
            .map(|number| text(id::<workspace_model::RequestId>(number)))
            .to_vec(),
    }).await.unwrap().rows
}

#[tokio::test]
async fn new_refused_and_missing_requests_store_exactly_their_typed_receipts_and_replay_them() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let (item, group, missing) = (probe_id(9400), probe_id(9401), probe_id(9402));
    seed_probe(&runtime, item, group, None, "receipts").await;
    let run = probe_request(9410, item, ProbeEvent::Run);
    let refused = probe_request(9411, item, ProbeEvent::Stop);
    let absent = probe_request(9412, missing, ProbeEvent::Run);

    let applied = runtime
        .request_probe(&fixture.caller, run.clone())
        .await
        .unwrap();
    let after_applied = row_counts(&runtime).await;
    let refusal = runtime
        .request_probe(&fixture.caller, refused.clone())
        .await
        .unwrap();
    let after_refusal = row_counts(&runtime).await;
    let not_found = runtime
        .request_probe(&fixture.caller, absent.clone())
        .await
        .unwrap();
    let after_not_found = row_counts(&runtime).await;

    assert_eq!(
        applied,
        EngineRequestOutcome::Answer {
            receipt: EngineReceipt::Applied {
                state: ProbeState::Fired
            }
        }
    );
    assert_eq!(
        refusal,
        EngineRequestOutcome::Answer {
            receipt: EngineReceipt::Refused {
                error: EngineRefusal::Domain {
                    error: ProbeRefusal::AlreadyFinished
                }
            }
        },
        "a refused request stores only its typed refusal"
    );
    assert_eq!(
        not_found,
        EngineRequestOutcome::Answer {
            receipt: EngineReceipt::NotFound
        }
    );
    assert_eq!(
        after_applied,
        vec![vec![
            integer(1),
            text("fired"),
            integer(1),
            integer(1),
            integer(0)
        ]],
        "one item change, one effect, one receipt"
    );
    assert_eq!(
        after_refusal,
        vec![vec![
            integer(1),
            text("fired"),
            integer(1),
            integer(2),
            integer(0)
        ]],
        "the refusal adds only its receipt"
    );
    assert_eq!(
        after_not_found,
        vec![vec![
            integer(1),
            text("fired"),
            integer(1),
            integer(3),
            integer(0)
        ]],
        "a missing subject adds only its NotFound receipt"
    );
    for (request, original) in [(run, applied), (refused, refusal), (absent, not_found)] {
        assert_eq!(
            runtime
                .request_probe(&fixture.caller, request)
                .await
                .unwrap(),
            original,
            "the replay decodes the stored typed answer"
        );
    }
    assert_eq!(
        row_counts(&runtime).await,
        after_not_found,
        "replays write nothing"
    );
    assert_eq!(
        runtime
            .request_probe(&fixture.caller, probe_request(9410, item, ProbeEvent::Stop))
            .await
            .unwrap(),
        EngineRequestOutcome::Answer {
            receipt: EngineReceipt::RequestIdReused
        }
    );
    assert_eq!(
        row_counts(&runtime).await,
        after_not_found,
        "a reused ID stores nothing"
    );
    fixture.host.close().await;
}

#[tokio::test]
async fn an_undecodable_stored_receipt_is_corrupt_and_changes_nothing() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let (item, group) = (probe_id(9420), probe_id(9421));
    seed_probe(&runtime, item, group, None, "undecodable").await;
    let request = probe_request(9422, item, ProbeEvent::Run);
    runtime
        .request_probe(&fixture.caller, request.clone())
        .await
        .unwrap();
    raw(
        &runtime,
        "UPDATE runtime_request_log SET answer='{\"receipt\":\"unknownShape\"}' WHERE request_id=?",
        vec![text(id::<workspace_model::RequestId>(9422))],
    )
    .await;
    let before = row_counts(&runtime).await;

    let replay = runtime.request_probe(&fixture.caller, request).await;

    assert_eq!(
        replay,
        Err(StoreFailure::Corrupt {
            reason: CorruptReason::BadValue {
                column: "runtime_request_log.answer".to_owned()
            }
        })
    );
    assert_eq!(row_counts(&runtime).await, before);
    fixture.host.close().await;
}

#[tokio::test]
async fn the_failure_table_has_the_p2_columns_and_indexes_no_check_and_uuid7_keys() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let columns = runtime.statement_host().read(Statement {
        sql: "SELECT name,type,\"notnull\",pk FROM pragma_table_info('engine_work_attempts') ORDER BY cid",
        args: vec![],
    }).await.unwrap().rows;
    let column = |name: &str, kind: &str, not_null: i64, pk: i64| {
        vec![text(name), text(kind), integer(not_null), integer(pk)]
    };
    assert_eq!(
        columns,
        vec![
            column("attempt_id", "TEXT", 0, 1),
            column("machine_kind", "TEXT", 1, 0),
            column("subject_key", "TEXT", 1, 0),
            column("state", "TEXT", 1, 0),
            column("failure_count", "INTEGER", 1, 0),
            column("next_try_at", "TEXT", 0, 0),
            column("last_failure", "TEXT", 1, 0),
            column("last_failure_detail", "TEXT", 0, 0),
            column("first_failed_at", "TEXT", 1, 0),
            column("last_failed_at", "TEXT", 1, 0),
            column("retired_at", "TEXT", 0, 0),
            column("created_at", "TEXT", 1, 0),
            column("updated_at", "TEXT", 1, 0),
        ]
    );
    let indexes = runtime.statement_host().read(Statement {
        sql: "SELECT name,\"unique\",(SELECT group_concat(name) FROM (SELECT name FROM pragma_index_info(l.name) ORDER BY seqno)) FROM pragma_index_list('engine_work_attempts') l WHERE origin='c' ORDER BY name",
        args: vec![],
    }).await.unwrap().rows;
    assert_eq!(
        indexes,
        vec![
            vec![
                text("central_work_attempts_state"),
                integer(0),
                text("machine_kind,state,next_try_at")
            ],
            vec![
                text("central_work_attempts_subject"),
                integer(1),
                text("machine_kind,subject_key")
            ],
        ]
    );
    let table = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT sql FROM sqlite_master WHERE name='engine_work_attempts'",
            args: vec![],
        })
        .await
        .unwrap()
        .rows;
    let SqlValue::Text(sql) = &table[0][0] else {
        panic!("table sql")
    };
    assert!(!sql.to_uppercase().contains("CHECK"), "{sql}");

    // A real failure record: its key is a UUIDv7.
    let due = now(&fixture) + chrono::Duration::minutes(5);
    let (item, group) = (probe_id(9430), probe_id(9431));
    seed_probe(&runtime, item, group, Some(due), "schema").await;
    raw(
        &runtime,
        "UPDATE engine_probe_items SET state='damaged' WHERE item_id=?",
        vec![text(item)],
    )
    .await;
    set_now(&fixture, due);
    runtime.after_event().await.unwrap();
    let ids = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT attempt_id FROM engine_work_attempts",
            args: vec![],
        })
        .await
        .unwrap()
        .rows;
    assert_eq!(ids.len(), 1);
    let SqlValue::Text(id) = &ids[0][0] else {
        panic!("attempt id")
    };
    assert_eq!(id.parse::<uuid::Uuid>().unwrap().get_version_num(), 7);
    fixture.host.close().await;
}
