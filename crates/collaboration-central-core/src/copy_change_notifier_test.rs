use super::CopyChangeNotifier;
use durable_steps::Uuid;
use std::sync::Arc;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[test]
fn a_commit_between_subscribe_and_wait_remains_ready_for_every_reader()
-> Result<(), Box<dyn std::error::Error>> {
    let changes = Arc::new(CopyChangeNotifier::default());
    let mut first = changes
        .subscribe(Uuid::now_v7())
        .map_err(|error| format!("{error:?}"))?;
    let mut second = changes
        .subscribe(Uuid::now_v7())
        .map_err(|error| format!("{error:?}"))?;

    changes.signal_committed();

    assert_eq!(first.changed.try_recv()?, Some(()));
    assert_eq!(second.changed.try_recv()?, Some(()));
    assert!(changes.pending.lock().map_err(|_| "poisoned")?.is_empty());
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[test]
fn instance_loss_cancels_the_wait_without_reporting_a_change()
-> Result<(), Box<dyn std::error::Error>> {
    let changes = Arc::new(CopyChangeNotifier::default());
    let mut waiter = changes
        .subscribe(Uuid::now_v7())
        .map_err(|error| format!("{error:?}"))?;

    drop(changes);

    assert!(waiter.changed.try_recv().is_err());
    Ok(())
}
