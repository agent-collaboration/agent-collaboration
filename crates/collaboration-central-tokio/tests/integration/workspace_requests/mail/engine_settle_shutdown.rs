//! Host close cancels a drive the alarm driver started; a test waiting for
//! in-flight drives to settle must not wait on it forever.
use super::super::support::*;
use super::engine_probe_support::*;
use collaboration_central_core::PausePoint;
use router_clock::RuntimeClock;

#[tokio::test]
async fn a_drive_cancelled_by_host_close_no_longer_counts_as_in_flight() {
    // Arrange: due work, and the driver's drive for it held mid-batch.
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let due = fixture.clock.utc_now() + chrono::Duration::minutes(5);
    seed_probe(
        &runtime,
        probe_id(13300),
        probe_id(13301),
        Some(due),
        "settle",
    )
    .await;
    runtime.after_event().await.unwrap();
    assert_eq!(
        runtime.statement_host().alarm().map(|at| at.unix_millis()),
        Some(due.timestamp_millis())
    );
    let mut held = runtime
        .pause_points()
        .hold_engine(PausePoint::BeforeControlClear, "legacy_workspace_drains");
    fixture.set_utc(due).await;
    held.until_held().await;
    let waiter = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.until_alarm_drives_settled().await }
    });
    runtime.until_settle_waiting().await;

    // Act: close the host while the runtime is still held here.
    fixture.host.close().await;

    // Assert: the cancelled drive is settled at once, and the waiter wakes.
    tokio::select! {
        biased;
        () = runtime.until_alarm_drives_settled() => {}
        () = std::future::ready(()) => {
            panic!("a drive cancelled by host close still counts as in flight")
        }
    }
    waiter.await.unwrap();
    drop(held);
}
