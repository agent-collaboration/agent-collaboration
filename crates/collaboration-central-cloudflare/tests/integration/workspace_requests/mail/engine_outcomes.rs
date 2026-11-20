//! V3: superseded, constraint, uncertain and store-down outcomes on the workerd host.
use super::super::engine_evidence::{
    assert_fixture_attempt_ids_are_uuid_v7, witness_fixture_attempt_ids,
};
use super::super::support::*;
use super::engine_probe_support::*;
use collaboration_central_core::engine_probe::{
    EngineReceipt, EngineRequestOutcome, ProbeEvent, ProbeState,
};
use fleet_runtime::{EngineCommitFault, EngineCommitFaultKind, PausePoint};

fn minutes(count: i64) -> chrono::Duration {
    chrono::Duration::minutes(count)
}

#[tokio::test]
async fn a_held_due_loses_its_compare_and_set_to_a_stop_request_and_ends_superseded() {
    let fixture = Fixture::new().await;
    witness_fixture_attempt_ids(&fixture).await;
    let runtime = runtime(&fixture).await;
    let due = now(&fixture) + minutes(5);
    let (item, group) = (probe_id(9000), probe_id(9001));
    seed_probe(&runtime, item, group, Some(due), "held").await;
    let mut held = runtime
        .pause_points()
        .hold_engine(PausePoint::AfterEngineLoad, &item.to_string())
        .await;
    set_now(&fixture, due);
    let batch = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.after_event().await }
    });
    held.until_held().await;

    let stop = runtime
        .request_probe(&fixture.caller, probe_request(9002, item, ProbeEvent::Stop))
        .await
        .unwrap();
    held.release().await;
    batch.await.unwrap().unwrap();

    assert_eq!(
        stop,
        EngineRequestOutcome::Answer {
            receipt: EngineReceipt::Applied {
                state: ProbeState::Stopped
            }
        }
    );
    assert_eq!(
        probe_facts(&runtime, item, group).await,
        vec![text("stopped"), integer(0), integer(0)],
        "the stale Due reloads, finds the item stopped and records nothing"
    );
    assert_fixture_attempt_ids_are_uuid_v7(
        &fixture,
        "a_held_due_loses_its_compare_and_set_to_a_stop_request_and_ends_superseded",
        &[],
    )
    .await;
    fixture.host.close().await;
}

async fn race_one_request_id(
    winner_first: bool,
    same_content: bool,
) -> (
    ProbeAnswer,
    ProbeAnswer,
    Vec<Vec<central_storage::SqlValue>>,
) {
    let fixture = Fixture::new().await;
    witness_fixture_attempt_ids(&fixture).await;
    let runtime = runtime(&fixture).await;
    let group = probe_id(9011);
    let (first_item, second_item) = (probe_id(9012), probe_id(9013));
    seed_probe(&runtime, first_item, group, None, "first").await;
    seed_probe(&runtime, second_item, group, None, "second").await;
    let second_target = if same_content {
        first_item
    } else {
        second_item
    };
    let request_key = id::<workspace_model::RequestId>(9010).to_string();
    let points = runtime.pause_points();
    let mut first_hold = points
        .hold_engine(PausePoint::AfterEngineAbsentLookup, &request_key)
        .await;
    let mut second_hold = points
        .hold_engine(PausePoint::AfterEngineAbsentLookup, &request_key)
        .await;
    let send = |target| {
        let runtime = runtime.clone();
        let caller = fixture.caller.clone();
        tokio::spawn(async move {
            runtime
                .request_probe(&caller, probe_request(9010, target, ProbeEvent::Run))
                .await
                .unwrap()
        })
    };
    let first = send(first_item);
    first_hold.until_held().await;
    let second = send(second_target);
    second_hold.until_held().await;

    let (first, second) = if winner_first {
        first_hold.release().await;
        let first = first.await.unwrap();
        second_hold.release().await;
        (first, second.await.unwrap())
    } else {
        second_hold.release().await;
        let second = second.await.unwrap();
        first_hold.release().await;
        (first.await.unwrap(), second)
    };
    let rows = runtime.statement_host().read(super::super::Statement {
        sql: "SELECT (SELECT effect_count FROM engine_probe_groups WHERE group_id=?),(SELECT COUNT(*) FROM runtime_request_log WHERE request_id=?),(SELECT COUNT(*) FROM engine_probe_items WHERE group_id=? AND state='fired')",
        args: vec![text(group), text(id::<workspace_model::RequestId>(9010)), text(group)],
    }).await.unwrap().rows;
    assert_fixture_attempt_ids_are_uuid_v7(
        &fixture,
        "racing_requests_with_one_id_commit_once_in_either_release_order",
        &[],
    )
    .await;
    fixture.host.close().await;
    (first, second, rows)
}

#[tokio::test]
async fn racing_requests_with_one_id_commit_once_in_either_release_order() {
    let fired = EngineRequestOutcome::Answer {
        receipt: EngineReceipt::Applied {
            state: ProbeState::Fired,
        },
    };
    let reused = EngineRequestOutcome::Answer {
        receipt: EngineReceipt::RequestIdReused,
    };
    for winner_first in [true, false] {
        let (first, second, rows) = race_one_request_id(winner_first, true).await;
        assert_eq!((&first, &second), (&fired, &fired), "same content replays");
        assert_eq!(rows, vec![vec![integer(1), integer(1), integer(1)]]);

        let (first, second, rows) = race_one_request_id(winner_first, false).await;
        let (winner, loser) = if winner_first {
            (first, second)
        } else {
            (second, first)
        };
        assert_eq!(winner, fired, "the committed request is the winner");
        assert_eq!(loser, reused, "different content is refused");
        assert_eq!(rows, vec![vec![integer(1), integer(1), integer(1)]]);
    }
}

#[tokio::test]
async fn a_second_request_after_the_winner_committed_replays_or_is_refused() {
    let fixture = Fixture::new().await;
    witness_fixture_attempt_ids(&fixture).await;
    let runtime = runtime(&fixture).await;
    let group = probe_id(9021);
    let (item, other) = (probe_id(9022), probe_id(9023));
    seed_probe(&runtime, item, group, None, "winner").await;
    seed_probe(&runtime, other, group, None, "other").await;
    let mut committed = runtime
        .pause_points()
        .hold_engine(PausePoint::AfterEngineCommit, &item.to_string())
        .await;
    let winner = tokio::spawn({
        let runtime = runtime.clone();
        let caller = fixture.caller.clone();
        async move {
            runtime
                .request_probe(&caller, probe_request(9020, item, ProbeEvent::Run))
                .await
                .unwrap()
        }
    });
    committed.until_held().await;

    let replay = runtime
        .request_probe(&fixture.caller, probe_request(9020, item, ProbeEvent::Run))
        .await
        .unwrap();
    let reused = runtime
        .request_probe(&fixture.caller, probe_request(9020, other, ProbeEvent::Run))
        .await
        .unwrap();
    committed.release().await;
    let winner = winner.await.unwrap();

    assert_eq!(replay, winner);
    assert_eq!(
        reused,
        EngineRequestOutcome::Answer {
            receipt: EngineReceipt::RequestIdReused
        }
    );
    assert_eq!(
        probe_facts(&runtime, other, group).await,
        vec![text("waiting"), integer(1), integer(0)]
    );
    assert_eq!(receipts(&runtime, 9020).await, 1);
    assert_fixture_attempt_ids_are_uuid_v7(
        &fixture,
        "a_second_request_after_the_winner_committed_replays_or_is_refused",
        &[],
    )
    .await;
    fixture.host.close().await;
}

async fn rejected_due_with_optional_stop(
    stop_before_failure_write: bool,
) -> Vec<central_storage::SqlValue> {
    let fixture = Fixture::new().await;
    witness_fixture_attempt_ids(&fixture).await;
    let runtime = runtime(&fixture).await;
    let due = now(&fixture) + minutes(5);
    let (item, group) = (probe_id(9030), probe_id(9031));
    seed_probe(&runtime, item, group, Some(due), "rejected").await;
    raw(
        &runtime,
        "CREATE TRIGGER probe_reject BEFORE UPDATE OF effect_count ON engine_probe_groups BEGIN SELECT RAISE(ABORT,'probe rejected'); END",
        vec![],
    )
    .await;
    let mut before_write = runtime
        .pause_points()
        .hold_engine(PausePoint::BeforeEngineFailureWrite, &item.to_string())
        .await;
    set_now(&fixture, due);
    let batch = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.after_event().await }
    });
    before_write.until_held().await;
    if stop_before_failure_write {
        assert!(matches!(
            runtime
                .request_probe(&fixture.caller, probe_request(9032, item, ProbeEvent::Stop))
                .await
                .unwrap(),
            EngineRequestOutcome::Answer {
                receipt: EngineReceipt::Applied {
                    state: ProbeState::Stopped
                }
            }
        ));
    }
    before_write.release().await;
    batch.await.unwrap().unwrap();
    let facts = probe_facts(&runtime, item, group).await;
    let mut rows = facts;
    rows.extend(attempt(&runtime, item).await.into_iter().flatten());
    assert_fixture_attempt_ids_are_uuid_v7(
        &fixture,
        "a_finishing_request_beats_a_stale_failure_write_and_its_control_records",
        &[],
    )
    .await;
    fixture.host.close().await;
    rows
}

#[tokio::test]
async fn a_finishing_request_beats_a_stale_failure_write_and_its_control_records() {
    assert_eq!(
        rejected_due_with_optional_stop(true).await,
        vec![text("stopped"), integer(0), integer(0)],
        "the guarded failure write finds the item finished: Superseded, no record"
    );
    assert_eq!(
        rejected_due_with_optional_stop(false).await,
        vec![
            text("waiting"),
            integer(0),
            integer(1),
            text("failing"),
            integer(1),
            text("statement_rejected")
        ],
        "with no Stop the same rejection is recorded"
    );
}

#[tokio::test]
async fn a_due_unique_violation_records_only_its_item_and_the_others_commit() {
    let fixture = Fixture::new().await;
    witness_fixture_attempt_ids(&fixture).await;
    let runtime = runtime(&fixture).await;
    let due = now(&fixture) + minutes(5);
    let group = probe_id(9040);
    let items = [probe_id(9041), probe_id(9042), probe_id(9043)];
    seed_probe(&runtime, items[0], group, None, "same claim").await;
    seed_probe(&runtime, items[1], group, Some(due), "same claim").await;
    seed_probe(&runtime, items[2], group, Some(due), "independent").await;
    assert!(matches!(
        runtime
            .request_probe(
                &fixture.caller,
                probe_request(9044, items[0], ProbeEvent::Run)
            )
            .await
            .unwrap(),
        EngineRequestOutcome::Answer { .. }
    ));
    set_now(&fixture, due);

    runtime.after_event().await.unwrap();

    assert_eq!(
        probe_facts(&runtime, items[0], group).await,
        vec![text("fired"), integer(2), integer(0)]
    );
    assert_eq!(
        probe_facts(&runtime, items[2], group).await,
        vec![text("fired"), integer(2), integer(0)]
    );
    assert_eq!(
        probe_facts(&runtime, items[1], group).await,
        vec![text("waiting"), integer(2), integer(1)]
    );
    assert_eq!(
        attempt(&runtime, items[1]).await,
        vec![vec![
            text("failing"),
            integer(1),
            text("constraint_violated")
        ]]
    );
    assert_fixture_attempt_ids_are_uuid_v7(
        &fixture,
        "a_due_unique_violation_records_only_its_item_and_the_others_commit",
        &[],
    )
    .await;
    fixture.host.close().await;
}

#[tokio::test]
async fn a_lost_commit_answer_is_uncertain_unrecorded_and_never_doubled() {
    for (number, kind, fired_first) in [
        (9050, EngineCommitFaultKind::AppliedAnswerLost, true),
        (9055, EngineCommitFaultKind::NotAppliedAnswerLost, false),
    ] {
        let fixture = Fixture::new().await;
        witness_fixture_attempt_ids(&fixture).await;
        let runtime = runtime(&fixture).await;
        let due = now(&fixture) + minutes(5);
        let (item, group) = (probe_id(number), probe_id(number + 1));
        seed_probe(&runtime, item, group, Some(due), "uncertain").await;
        runtime
            .inject_engine_commit_fault(EngineCommitFault {
                key: key(item),
                kind: kind.clone(),
            })
            .await;
        set_now(&fixture, due);

        runtime.after_event().await.unwrap();

        let state = if fired_first { "fired" } else { "waiting" };
        assert_eq!(
            probe_facts(&runtime, item, group).await,
            vec![text(state), integer(i64::from(fired_first)), integer(0)],
            "{kind:?}: counted uncertain, no record"
        );
        set_now(&fixture, due + minutes(5));
        runtime.after_event().await.unwrap();
        assert_eq!(
            probe_facts(&runtime, item, group).await,
            vec![text("fired"), integer(1), integer(0)],
            "{kind:?}: the next pass finishes it, the effect exists once"
        );
        assert_fixture_attempt_ids_are_uuid_v7(
            &fixture,
            "a_lost_commit_answer_is_uncertain_unrecorded_and_never_doubled",
            &[],
        )
        .await;
        fixture.host.close().await;
    }
}

#[tokio::test]
async fn operational_database_codes_are_never_charged_to_the_item() {
    for (number, code) in [(9060, 13), (9063, 778), (9066, 5), (9069, 99_999)] {
        let fixture = Fixture::new().await;
        witness_fixture_attempt_ids(&fixture).await;
        let runtime = runtime(&fixture).await;
        let due = now(&fixture) + minutes(5);
        let (item, group) = (probe_id(number), probe_id(number + 1));
        seed_probe(&runtime, item, group, Some(due), "operational").await;
        runtime
            .inject_engine_commit_fault(EngineCommitFault {
                key: key(item),
                kind: EngineCommitFaultKind::DatabaseCode(code),
            })
            .await;
        set_now(&fixture, due);

        runtime.after_event().await.unwrap();

        assert_eq!(
            probe_facts(&runtime, item, group).await,
            vec![text("waiting"), integer(0), integer(0)],
            "code {code} is an outage, not the item's failure"
        );
        let floor = due + chrono::Duration::seconds(30);
        assert_eq!(
            alarm_ms(&runtime).await,
            Some(floor.timestamp_millis()),
            "code {code}"
        );
        set_now(&fixture, floor);
        runtime.after_event().await.unwrap();
        assert_eq!(
            probe_facts(&runtime, item, group).await,
            vec![text("fired"), integer(1), integer(0)],
            "code {code}: worked normally when the alarm fires"
        );
        assert_fixture_attempt_ids_are_uuid_v7(
            &fixture,
            "operational_database_codes_are_never_charged_to_the_item",
            &[],
        )
        .await;
        fixture.host.close().await;
    }
}

#[tokio::test]
async fn no_unit_starts_after_central_store_down_and_started_units_settle() {
    // Seven earlier units are held mid-handle so they are in flight on both hosts
    // (workerd storage calls do not yield), then the eighth hits the outage.
    let fixture = Fixture::new().await;
    witness_fixture_attempt_ids(&fixture).await;
    let runtime = runtime(&fixture).await;
    let due = now(&fixture) + minutes(5);
    let group = probe_id(9100);
    let faulted = probe_id(9101);
    seed_probe(
        &runtime,
        faulted,
        group,
        Some(due - chrono::Duration::seconds(13)),
        "faulted",
    )
    .await;
    let in_flight: Vec<_> = (0..7).map(|offset| probe_id(9110 + offset)).collect();
    let later: Vec<_> = (0..12).map(|offset| probe_id(9130 + offset)).collect();
    for (offset, item) in in_flight.iter().enumerate() {
        let at = due - chrono::Duration::seconds(20 - i64::try_from(offset).unwrap());
        seed_probe(&runtime, *item, group, Some(at), &format!("flight{offset}")).await;
    }
    for (offset, item) in later.iter().enumerate() {
        seed_probe(&runtime, *item, group, Some(due), &format!("later{offset}")).await;
    }
    let points = runtime.pause_points();
    let mut holds = Vec::new();
    for item in &in_flight {
        holds.push(
            points
                .hold_engine(PausePoint::AfterEngineLoad, &item.to_string())
                .await,
        );
    }
    runtime
        .inject_engine_commit_fault(EngineCommitFault {
            key: key(faulted),
            kind: EngineCommitFaultKind::Unavailable,
        })
        .await;
    set_now(&fixture, due);
    let batch = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.after_event().await }
    });
    for hold in &mut holds {
        hold.until_held().await;
    }
    while runtime.statement_host().engine_commit_fault_pending().await {
        tokio::task::yield_now().await;
    }
    for hold in holds {
        hold.release().await;
    }

    let completion = batch.await.unwrap();

    assert!(
        completion.is_ok(),
        "the final publication is attempted: {completion:?}"
    );
    for item in &in_flight {
        assert_eq!(
            probe_facts(&runtime, *item, group).await[0],
            text("fired"),
            "units already started settle"
        );
    }
    for item in &later {
        assert_eq!(
            probe_facts(&runtime, *item, group).await[0],
            text("waiting"),
            "no unit starts after the outage"
        );
    }
    assert_eq!(
        probe_facts(&runtime, faulted, group).await,
        vec![text("waiting"), integer(7), integer(0)],
        "the faulted item carries no record"
    );
    assert_fixture_attempt_ids_are_uuid_v7(
        &fixture,
        "no_unit_starts_after_central_store_down_and_started_units_settle",
        &[],
    )
    .await;
    fixture.host.close().await;
}

#[tokio::test]
async fn one_held_unit_never_delays_the_other_nineteen_or_a_probe_request() {
    let fixture = Fixture::new().await;
    witness_fixture_attempt_ids(&fixture).await;
    let runtime = runtime(&fixture).await;
    let due = now(&fixture) + minutes(5);
    let group = probe_id(9200);
    let items: Vec<_> = (0..20).map(|offset| probe_id(9201 + offset)).collect();
    for (offset, item) in items.iter().enumerate() {
        seed_probe(&runtime, *item, group, Some(due), &format!("unit{offset}")).await;
    }
    let other = probe_id(9230);
    seed_probe(&runtime, other, group, None, "request").await;
    let mut held = runtime
        .pause_points()
        .hold_engine(PausePoint::AfterEngineLoad, &items[0].to_string())
        .await;
    set_now(&fixture, due);
    let batch = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.after_event().await }
    });
    held.until_held().await;
    let mut committed = 0;
    while committed < 19 {
        tokio::task::yield_now().await;
        committed = 0;
        for item in &items[1..] {
            if probe_facts(&runtime, *item, group).await[0] == text("fired") {
                committed += 1;
            }
        }
    }
    let answer = runtime
        .request_probe(&fixture.caller, probe_request(9231, other, ProbeEvent::Run))
        .await
        .unwrap();
    assert!(matches!(answer, EngineRequestOutcome::Answer { .. }));
    assert_eq!(
        probe_facts(&runtime, items[0], group).await[0],
        text("waiting")
    );
    held.release().await;
    batch.await.unwrap().unwrap();
    assert_eq!(
        probe_facts(&runtime, items[0], group).await[0],
        text("fired")
    );
    assert_fixture_attempt_ids_are_uuid_v7(
        &fixture,
        "one_held_unit_never_delays_the_other_nineteen_or_a_probe_request",
        &[],
    )
    .await;
    fixture.host.close().await;
}
