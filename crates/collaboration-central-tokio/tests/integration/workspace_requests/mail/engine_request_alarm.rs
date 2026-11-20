//! R7 on the native host (code review C1): a request that changes a timed
//! subject's due state re-publishes the alarm in its own atomic batch, so the
//! physical alarm has moved by the time the commit returns, before the
//! request's closing due batch runs.
use super::super::engine_evidence::{
    assert_fixture_attempt_ids_are_uuid_v7, witness_fixture_attempt_ids,
};
use super::super::support::*;
use super::engine_probe_support::*;
use collaboration_central_core::engine_probe::{EngineRequestOutcome, ProbeEvent};
use fleet_runtime::PausePoint;
use router_clock::RuntimeClock;

#[tokio::test]
async fn a_request_that_stops_the_earliest_timed_subject_moves_the_alarm_with_its_commit() {
    let fixture = Fixture::new().await;
    witness_fixture_attempt_ids(&fixture).await;
    let runtime = runtime(&fixture).await;
    let now = fixture.clock.utc_now();
    let earliest = now + chrono::Duration::minutes(10);
    let later = now + chrono::Duration::minutes(20);
    let group = probe_id(9800);
    let stopped = probe_id(9801);
    seed_probe(&runtime, stopped, group, Some(earliest), "earliest").await;
    seed_probe(&runtime, probe_id(9802), group, Some(later), "later").await;
    runtime.after_event().await.unwrap();
    let before = alarm_ms(&runtime).expect("the earliest probe arms the alarm");

    // The Stop request commits and is held at the post-commit pause, before
    // its closing due batch and publication.
    let mut committed = runtime
        .runtime()
        .pause_points()
        .hold_engine(PausePoint::AfterEngineCommit, &stopped.to_string());
    let request = tokio::spawn({
        let runtime = runtime.clone();
        let caller = fixture.caller.clone();
        async move {
            runtime
                .request_probe(&caller, probe_request(9803, stopped, ProbeEvent::Stop))
                .await
                .unwrap()
        }
    });
    committed.until_held().await;
    let at_commit = alarm_ms(&runtime);

    committed.release();
    let answer = request.await.unwrap();
    assert!(
        matches!(answer, EngineRequestOutcome::Answer { .. }),
        "{answer:?}"
    );
    let settled = alarm_ms(&runtime);
    assert_ne!(
        settled,
        Some(before),
        "stopping the earliest subject moves the alarm"
    );
    assert_eq!(
        at_commit, settled,
        "the request's own batch already published the alarm the event settles on"
    );
    assert_fixture_attempt_ids_are_uuid_v7(
        &fixture,
        "a_request_that_stops_the_earliest_timed_subject_moves_the_alarm_with_its_commit",
        &[],
    )
    .await;
    fixture.host.close().await;
}
