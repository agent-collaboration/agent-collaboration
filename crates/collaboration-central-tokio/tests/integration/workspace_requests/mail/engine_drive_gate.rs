//! The gated clock: native alarm drives and fixture clock moves exclude each
//! other, so a drive never takes its batch time on one side of a move and
//! publishes on the other.
use super::super::support::*;
use super::engine_probe_support::*;
use fleet_contracts::WorkspaceCaller;

use fleet_runtime::PausePoint;
use router_clock::RuntimeClock;
use workspace_model as m;

/// Seeds due probe work and publishes its alarm, so the native driver wakes at `due`.
async fn arm(runtime: &Runtime, number: u32, due: chrono::DateTime<chrono::Utc>) {
    seed_probe(
        runtime,
        probe_id(number),
        probe_id(number + 1),
        Some(due),
        "gate",
    )
    .await;
    runtime.after_event().await.unwrap();
    assert_eq!(
        runtime
            .runtime()
            .statement_host()
            .alarm()
            .map(|at| at.unix_millis()),
        Some(due.timestamp_millis())
    );
}

#[tokio::test]
async fn a_drive_that_wakes_during_a_held_move_waits_for_it() {
    // Arrange: one organization with work due at `due`.
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let due = fixture.clock.utc_now() + chrono::Duration::minutes(5);
    arm(&runtime, 13400, due).await;
    let completed = runtime.runtime().alarm_completion_count();
    let starts = runtime.runtime().due_batch_starts().len();

    // Act: hold a move and, inside it, reach `due`; the driver wakes.
    let held = fixture.clock.hold_move().await;
    held.advance((due - fixture.clock.utc_now()).to_std().unwrap())
        .unwrap();
    tokio::select! {
        biased;
        () = runtime.runtime().until_alarm_completed(completed + 1) => {
            panic!("a drive ran during a move")
        }
        () = fixture.clock.until_drive_waiting() => {}
    }
    assert_eq!(runtime.runtime().due_batch_starts().len(), starts);
    held.advance(std::time::Duration::from_secs(10)).unwrap();
    drop(held);

    // Assert: the drive ran after the move and took its time from after it.
    runtime.runtime().until_alarm_completed(completed + 1).await;
    assert_eq!(
        runtime.runtime().due_batch_starts().last(),
        Some(&(due + chrono::Duration::seconds(10)).timestamp_millis())
    );
    fixture.host.close().await;
}

#[tokio::test]
async fn a_move_waiting_on_one_organization_runs_before_another_organization_drives() {
    // Arrange: organizations A and B, both due at `due`, on one host and clock.
    let fixture = Fixture::new().await;
    let a = runtime(&fixture).await;
    let other_organization: fleet_identity_protocol::OrganizationId =
        id::<m::WorkspaceId>(91).to_string().try_into().unwrap();
    let other: WorkspaceCaller =
        admitted_organization(&fixture.host, &other_organization, fixture.clock.as_ref())
            .await
            .unwrap();
    let b = fixture.host.central(other.organization_id()).await.unwrap();
    let due = fixture.clock.utc_now() + chrono::Duration::minutes(5);
    arm(&a, 13410, due).await;
    arm(&b, 13420, due).await;
    let a_completed = a.runtime().alarm_completion_count();
    // A's driver wakes but stops before its drive guard: runnable from the old
    // position. B's drive takes its guard and stops mid-batch.
    let mut a_waiting = a
        .runtime()
        .pause_points()
        .hold_engine(PausePoint::BeforeDriveGuard, "driver");
    let mut b_held = b
        .runtime()
        .pause_points()
        .hold_engine(PausePoint::BeforeControlClear, "legacy_workspace_drains");
    fixture
        .advance((due - fixture.clock.utc_now()).to_std().unwrap())
        .await;
    a_waiting.until_held().await;
    b_held.until_held().await;

    // Act: move the clock while B's drive is held, then let A try to drive.
    let later = due + chrono::Duration::seconds(10);
    let mut moved = std::pin::pin!(fixture.advance(std::time::Duration::from_secs(10)));
    tokio::select! {
        biased;
        () = &mut moved => panic!("the clock moved while organization B's drive was held"),
        () = fixture.clock.until_move_waiting() => {}
    }
    a_waiting.release();
    tokio::select! {
        biased;
        () = a.runtime().until_alarm_completed(a_completed + 1) => {
            panic!("organization A drove while a move was waiting")
        }
        () = fixture.clock.until_drive_waiting() => {}
    }
    assert_eq!(fixture.clock.utc_now(), due);
    b_held.release();
    moved.await;

    // Assert: A's drive ran after the move and took its time from after it.
    a.runtime().until_alarm_completed(a_completed + 1).await;
    assert_eq!(fixture.clock.utc_now(), later);
    assert_eq!(
        a.runtime().due_batch_starts().last(),
        Some(&later.timestamp_millis())
    );
    fixture.host.close().await;
}
