//! V4 on the native host: the failure record's life through success, today's
//! finishing requests, failed batches and a damaged stored record.
use super::super::engine_evidence::{
    assert_every_written_attempt_id_is_uuid_v7, dump_attempts, install_attempt_id_witness,
};
use super::super::{Statement, support::*};
use super::engine_probe_support::*;
use central_storage::SqlValue;
use collaboration_central_core::{EngineCommitFault, EngineCommitFaultKind};
use router_clock::RuntimeClock;
use workspace_model::central_mail::{
    CentralMailAnswer as Answer, CentralMailOperation as Operation, CentralMailRequest,
    CentralMailResult as Result,
};

const REJECT_PROBE: &str = "CREATE TRIGGER probe_reject BEFORE UPDATE OF effect_count ON engine_probe_groups BEGIN SELECT RAISE(ABORT,'probe rejected'); END";

fn minutes(count: i64) -> chrono::Duration {
    chrono::Duration::minutes(count)
}

async fn record(runtime: &Runtime, kind: &str, key: &str) -> Vec<Vec<SqlValue>> {
    runtime.statement_host().read(Statement {
        sql: "SELECT state,failure_count,last_failure,last_failure_detail FROM engine_work_attempts WHERE machine_kind=? AND subject_key=?",
        args: vec![text(kind), text(key)],
    }).await.unwrap().rows
}
/// The R4 receipt: the whole table after one step of `test`.
async fn dump(runtime: &Runtime, test: &str, step: &str) {
    dump_attempts(runtime.statement_host(), &format!("r4-{test}.jsonl"), step).await;
}

#[tokio::test]
async fn a_success_deletes_the_record_in_its_batch_and_a_failed_success_batch_changes_nothing() {
    const TEST: &str =
        "a_success_deletes_the_record_in_its_batch_and_a_failed_success_batch_changes_nothing";
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    install_attempt_id_witness(runtime.statement_host()).await;
    let due = fixture.clock.utc_now() + minutes(5);
    let (item, group) = (probe_id(9500), probe_id(9501));
    seed_probe(&runtime, item, group, Some(due), "retry once").await;
    raw(&runtime, REJECT_PROBE, vec![]).await;
    dump(&runtime, TEST, "seeded, before any pass").await;
    fixture.set_utc(due).await;
    runtime.after_event().await.unwrap();
    dump(&runtime, TEST, "after the rejected pass").await;
    assert_eq!(
        attempt(&runtime, item).await,
        vec![vec![
            text("failing"),
            integer(1),
            text("statement_rejected")
        ]]
    );
    raw(&runtime, "DROP TRIGGER probe_reject", vec![]).await;

    // The success batch itself fails: the record and the subject stay as they were.
    runtime
        .statement_host()
        .inject_engine_commit_fault(EngineCommitFault {
            key: key(item),
            kind: EngineCommitFaultKind::Unavailable,
        });
    fixture.set_utc(due + minutes(5)).await;
    runtime.after_event().await.unwrap();
    dump(&runtime, TEST, "after the failed success batch").await;
    assert_eq!(
        probe_facts(&runtime, item, group).await,
        vec![text("waiting"), integer(0), integer(1)]
    );
    assert_eq!(
        attempt(&runtime, item).await,
        vec![vec![
            text("failing"),
            integer(1),
            text("statement_rejected")
        ]]
    );

    // The retry succeeds: the record is gone in the success's own batch.
    fixture
        .set_utc(due + minutes(5) + chrono::Duration::seconds(30))
        .await;
    runtime.after_event().await.unwrap();
    dump(&runtime, TEST, "after the successful retry").await;
    assert_eq!(
        probe_facts(&runtime, item, group).await,
        vec![text("fired"), integer(1), integer(0)]
    );
    assert!(
        assert_every_written_attempt_id_is_uuid_v7(runtime.statement_host(), TEST, &[]).await > 0
    );
    fixture.host.close().await;
}

async fn failing_wake(fixture: &Fixture, number: u32) -> (Runtime, workspace_ids::WakeId) {
    let runtime = runtime(fixture).await;
    install_attempt_id_witness(runtime.statement_host()).await;
    let due = fixture.clock.utc_now() + minutes(5);
    let Result::Done(Answer::WakeSet { wake_id, .. }) = fixture
        .host
        .request_central(
            &fixture.caller,
            CentralMailRequest {
                request_id: id(number),
                operation: Operation::SetWake {
                    target: session(&fixture.caller),
                    due_at: due,
                    text: "failing wake".to_owned().try_into().unwrap(),
                },
            },
        )
        .await
        .unwrap()
    else {
        panic!("wake refused")
    };
    raw(
        &runtime,
        "CREATE TRIGGER wake_reject BEFORE UPDATE OF state ON automation_wakes WHEN NEW.state='done' BEGIN SELECT RAISE(ABORT,'wake rejected'); END",
        vec![],
    )
    .await;
    fixture.set_utc(due).await;
    runtime.after_event().await.unwrap();
    raw(&runtime, "DROP TRIGGER wake_reject", vec![]).await;
    assert_eq!(
        record(&runtime, "wake_timer", wake_id.as_str()).await[0][..2],
        [text("failing"), integer(1)]
    );
    (runtime, wake_id)
}

async fn failing_automation(fixture: &Fixture, number: u32) -> (Runtime, String) {
    let runtime = runtime(fixture).await;
    install_attempt_id_witness(runtime.statement_host()).await;
    let Result::Done(Answer::AutomationSet { automation_id, .. }) = fixture
        .host
        .request_central(
            &fixture.caller,
            CentralMailRequest {
                request_id: id(number),
                operation: Operation::SetAutomation {
                    target: session(&fixture.caller),
                    timing: serde_json::from_str(r#"{"kind":"after","seconds":300}"#).unwrap(),
                    text: "failing automation".to_owned().try_into().unwrap(),
                },
            },
        )
        .await
        .unwrap()
    else {
        panic!("automation refused")
    };
    raw(
        &runtime,
        "CREATE TRIGGER automation_reject BEFORE UPDATE OF evaluated_through_at ON automation_schedules BEGIN SELECT RAISE(ABORT,'automation rejected'); END",
        vec![],
    )
    .await;
    fixture.set_utc(fixture.clock.utc_now() + minutes(5)).await;
    runtime.after_event().await.unwrap();
    raw(&runtime, "DROP TRIGGER automation_reject", vec![]).await;
    let automation_id = automation_id.to_string();
    assert_eq!(
        record(&runtime, "automation_timer", &automation_id).await[0][..2],
        [text("failing"), integer(1)]
    );
    (runtime, automation_id)
}

#[tokio::test]
async fn todays_cancel_and_disable_requests_delete_the_record_and_a_failed_request_changes_nothing()
{
    const TEST: &str =
        "todays_cancel_and_disable_requests_delete_the_record_and_a_failed_request_changes_nothing";
    // A failed CancelWake batch changes neither the wake nor its record.
    let fixture = Fixture::new().await;
    let (runtime, wake_id) = failing_wake(&fixture, 9510).await;
    dump(&runtime, TEST, "wake failing once").await;
    raw(
        &runtime,
        "CREATE TRIGGER cancel_reject BEFORE UPDATE OF state ON automation_wakes WHEN NEW.state='disabled' BEGIN SELECT RAISE(ABORT,'cancel rejected'); END",
        vec![],
    )
    .await;
    let cancel = CentralMailRequest {
        request_id: id(9511),
        operation: Operation::CancelWake {
            wake_id: wake_id.clone(),
        },
    };
    let failed = fixture.host.request_central(&fixture.caller, cancel).await;
    dump(&runtime, TEST, "after the rejected CancelWake").await;
    assert!(
        !matches!(failed, Ok(Result::Done(_))),
        "the rejected cancel must not succeed: {failed:?}"
    );
    let wake_state = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT state FROM automation_wakes WHERE wake_id=?",
            args: vec![text(&wake_id)],
        })
        .await
        .unwrap()
        .rows;
    assert_eq!(wake_state, vec![vec![text("active")]]);
    assert_eq!(
        record(&runtime, "wake_timer", wake_id.as_str()).await[0][..2],
        [text("failing"), integer(1)]
    );
    raw(&runtime, "DROP TRIGGER cancel_reject", vec![]).await;

    // A successful CancelWake disables the wake and deletes its record in that batch.
    assert!(matches!(
        fixture
            .host
            .request_central(
                &fixture.caller,
                CentralMailRequest {
                    request_id: id(9512),
                    operation: Operation::CancelWake {
                        wake_id: wake_id.clone()
                    },
                },
            )
            .await
            .unwrap(),
        Result::Done(_)
    ));
    dump(&runtime, TEST, "after the successful CancelWake").await;
    assert!(
        record(&runtime, "wake_timer", wake_id.as_str())
            .await
            .is_empty()
    );
    assert!(
        assert_every_written_attempt_id_is_uuid_v7(runtime.statement_host(), TEST, &[]).await > 0
    );
    fixture.host.close().await;

    let fixture = Fixture::new().await;
    let (runtime, automation_id) = failing_automation(&fixture, 9520).await;
    dump(&runtime, TEST, "automation failing once").await;
    raw(
        &runtime,
        "CREATE TRIGGER disable_reject BEFORE UPDATE OF state ON automation_schedules WHEN NEW.state='disabled' BEGIN SELECT RAISE(ABORT,'disable rejected'); END",
        vec![],
    )
    .await;
    let failed = fixture
        .host
        .request_central(
            &fixture.caller,
            CentralMailRequest {
                request_id: id(9521),
                operation: Operation::DisableAutomation {
                    automation_id: automation_id.parse().unwrap(),
                },
            },
        )
        .await;
    dump(&runtime, TEST, "after the rejected DisableAutomation").await;
    assert!(
        !matches!(failed, Ok(Result::Done(_))),
        "the rejected disable must not succeed: {failed:?}"
    );
    assert_eq!(
        record(&runtime, "automation_timer", &automation_id).await[0][..2],
        [text("failing"), integer(1)]
    );
    raw(&runtime, "DROP TRIGGER disable_reject", vec![]).await;
    assert!(matches!(
        fixture
            .host
            .request_central(
                &fixture.caller,
                CentralMailRequest {
                    request_id: id(9522),
                    operation: Operation::DisableAutomation {
                        automation_id: automation_id.parse().unwrap(),
                    },
                },
            )
            .await
            .unwrap(),
        Result::Done(_)
    ));
    dump(&runtime, TEST, "after the successful DisableAutomation").await;
    assert!(
        record(&runtime, "automation_timer", &automation_id)
            .await
            .is_empty()
    );
    assert!(
        assert_every_written_attempt_id_is_uuid_v7(runtime.statement_host(), TEST, &[]).await > 0
    );
    fixture.host.close().await;
}

#[tokio::test]
async fn a_damaged_record_is_replaced_once_by_a_corrupt_row_record_then_retires_and_is_never_visited()
 {
    const TEST: &str = "a_damaged_record_is_replaced_once_by_a_corrupt_row_record_then_retires_and_is_never_visited";
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    install_attempt_id_witness(runtime.statement_host()).await;
    let due = fixture.clock.utc_now() + minutes(5);
    let group = probe_id(9530);
    let (damaged, healthy) = (probe_id(9531), probe_id(9532));
    // The damaged record's subject keeps failing on its own (its group rejects
    // effects), so the replaced record follows the normal path to retirement.
    let broken_group = probe_id(9534);
    seed_probe(&runtime, damaged, broken_group, Some(due), "damaged").await;
    raw(
        &runtime,
        "CREATE TRIGGER broken_group BEFORE UPDATE OF effect_count ON engine_probe_groups WHEN OLD.group_id='01890f2e-7b4c-7cc0-98c4-000000009534' BEGIN SELECT RAISE(ABORT,'broken group'); END",
        vec![],
    )
    .await;
    seed_probe(&runtime, healthy, group, Some(due), "healthy").await;
    raw(
        &runtime,
        "INSERT INTO engine_work_attempts(attempt_id,machine_kind,subject_key,state,failure_count,next_try_at,last_failure,last_failure_detail,first_failed_at,last_failed_at,retired_at,created_at,updated_at) VALUES ('01890f2e-7b4c-7cc0-98c4-000000009533','engine_probe',?,'mangled',1,NULL,'corrupt_row',NULL,'2026-11-13T12:00:00.000Z','2026-11-13T12:00:00.000Z',NULL,'2026-11-13T12:00:00.000Z','2026-11-13T12:00:00.000Z')",
        vec![text(damaged)],
    )
    .await;
    dump(&runtime, TEST, "damaged record planted").await;
    fixture.set_utc(due).await;

    runtime.after_event().await.unwrap();
    dump(&runtime, TEST, "after pass 1").await;

    assert_eq!(
        probe_facts(&runtime, healthy, group).await[0],
        text("fired")
    );
    let replaced = record(&runtime, "engine_probe", &damaged.to_string()).await;
    assert_eq!(
        replaced,
        vec![vec![
            text("failing"),
            integer(1),
            text("corrupt_row"),
            text("engine_work_attempts.state\nmangled")
        ]],
        "replaced once by CorruptRow with the raw value kept; never defaulted"
    );
    fixture.set_utc(due + minutes(5)).await;
    runtime.after_event().await.unwrap();
    dump(&runtime, TEST, "after pass 2 at the next mark").await;
    assert_eq!(
        record(&runtime, "engine_probe", &damaged.to_string()).await[0][..2],
        [text("retired"), integer(2)]
    );
    let before = probe_facts(&runtime, damaged, broken_group).await;
    for step in 2..14 {
        fixture.set_utc(due + minutes(5 * step)).await;
        runtime.after_event().await.unwrap();
    }
    dump(&runtime, TEST, "after twelve later marks").await;
    assert_eq!(
        probe_facts(&runtime, damaged, broken_group).await,
        before,
        "a retired subject is never visited, even after its own due time"
    );
    assert_eq!(
        record(&runtime, "engine_probe", &damaged.to_string()).await[0][..2],
        [text("retired"), integer(2)]
    );
    assert!(
        assert_every_written_attempt_id_is_uuid_v7(
            runtime.statement_host(),
            TEST,
            &["01890f2e-7b4c-7cc0-98c4-000000009533"],
        )
        .await
            > 0
    );
    fixture.host.close().await;
}
