//! Host close cancels a drive the alarm driver started; the drive's guard on
//! the gated clock goes with it, so a waiting clock move never waits forever.
use super::super::support::*;
use super::engine_probe_support::*;
use fleet_runtime::PausePoint;
use router_clock::RuntimeClock;

#[tokio::test]
async fn a_drive_cancelled_by_host_close_releases_its_clock_guard() {
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
    let mut moved = std::pin::pin!(fixture.advance(std::time::Duration::from_secs(10)));
    tokio::select! {
        biased;
        () = &mut moved => panic!("the clock moved while a drive was held"),
        () = fixture.clock.until_move_waiting() => {}
    }

    // Act: close the host while the runtime and the pause are still held here.
    fixture.host.close().await;

    // Assert: the cancelled drive's guard is gone, so the move completes at once.
    tokio::select! {
        biased;
        () = &mut moved => {}
        () = std::future::ready(()) => {
            panic!("a drive cancelled by host close still holds its clock guard")
        }
    }
    drop(held);
}
