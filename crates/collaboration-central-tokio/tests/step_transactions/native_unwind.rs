use super::*;

#[allow(clippy::panic)]
fn unwind_after_fleet_writes() {
    panic!("native callback unwound after Fleet writes");
}

#[tokio::test]
async fn unwinding_step_rolls_back_all_writes_and_keeps_the_same_store_usable() -> TestResult {
    // Arrange: keep the original object/connection alive across the task unwind.
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), true).await?;
    let (entry, effects) = collaboration_central_conformance::receipt_with_effects_fixture(
        RequestId::try_from("019f0000-0000-7000-8000-0000000000a3".to_owned())?,
    )?;
    let callback_driver = StepConformance::new(
        Arc::new(TokioStorageHost::new(fixture.object.clone())),
        Arc::new(FixtureHost),
        runtime_primitives::Clock::now(&FixtureHost),
    );
    let offered = entry.clone();
    let offered_effects = effects.clone();

    // Act: an ordinary Rust callback unwinds inside StepDriver's transaction.
    let task = tokio::spawn(async move {
        callback_driver
            .record_with_native_callback(offered, offered_effects, unwind_after_fleet_writes)
            .await
    });
    let Err(unwound) = tokio::time::timeout(Duration::from_secs(10), task).await? else {
        return Err("the after-write callback did not unwind its task".into());
    };
    assert!(
        unwound.is_panic(),
        "callback task was cancelled instead of unwinding"
    );
    let payload = unwound.into_panic();
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&"native callback unwound after Fleet writes")
    );

    // Assert: no reopen, manual rollback or replacement connection intervenes.
    // The first read starts the store's normal BEGIN IMMEDIATE; it fails if the
    // unwound callback left a transaction open or the connection unusable.
    assert_eq!(
        tokio::time::timeout(
            Duration::from_secs(10),
            fixture.driver.read_fleet_decision(entry.request_id.clone())
        )
        .await??,
        None
    );
    assert_eq!(stored_receipt_count(&fixture).await?, "0");

    let answer = tokio::time::timeout(
        Duration::from_secs(10),
        fixture
            .driver
            .record_fleet_decision(entry.clone(), effects.clone()),
    )
    .await??;
    let [FleetRequestResult::Decided(RequestDecision::Accepted { effects: page, .. })] =
        answer.as_slice()
    else {
        return Err("the next transaction on the same store was not Accepted".into());
    };
    assert_eq!(page.entries(), effects.as_slice());
    assert_eq!(page.total_count(), 2);
    assert_eq!(stored_receipt_count(&fixture).await?, "2");
    assert_eq!(
        fixture
            .driver
            .read_fleet_decision(entry.request_id.clone())
            .await?,
        Some(entry)
    );
    fixture.objects.close().await?;
    Ok(())
}
