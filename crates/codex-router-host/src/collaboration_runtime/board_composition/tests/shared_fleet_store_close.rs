//! The Fleet client holds views of the Host's one Fleet database, so closing it
//! never closes the connection the link owner wrote through; the store's owner does.
use super::*;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn closing_admission_leaves_the_fleet_database_open_for_its_owner() -> TestResult {
    // Arrange: a configured Host board whose link owner and Fleet client share the store.
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("router");
    let mut service =
        TestFleet::start(&directory.path().join("central"), GateAnswer::Admitted).await?;
    let board = open_board_admission(&root, identity()?, Some(service.inputs()?)).await?;
    let ready = board
        .session
        .as_ref()
        .ok_or("session missing")?
        .welcomed
        .subscribe();
    let link = board.link_observation();
    let admission = board.admission.clone();
    let started = board.start();
    let running = started.session.ok_or("session not started")?;
    timeout(BOUND, service.auth.entered.notified()).await?;
    service.release()?;
    wait_welcome(ready).await?;

    // Act: the runtime's order. Stopping the session ends the link owner after its
    // final write; then admission, and with it the Fleet client, closes.
    running.shutdown().await?;
    let committed = link.read().await?.ok_or("link record missing")?;
    Arc::try_unwrap(admission)
        .map_err(|_| io::Error::other("admission still owned"))?
        .close()
        .await?;

    // Assert: the store is still open until its owner closes it, and holds exactly
    // the link owner's final record.
    assert_eq!(
        started.fleet_store.cache().fleet_link().await?,
        Some(committed)
    );
    started.fleet_store.close().await;
    service.stop().await?;
    Ok(())
}
