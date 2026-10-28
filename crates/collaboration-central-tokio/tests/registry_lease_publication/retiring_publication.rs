//! A public retirement seal has no grant; startup repair must still converge.
use super::*;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)] // Assertions pin the real public snapshot and alarm lifecycle.
async fn retiring_snapshot_reopens_and_publishes_without_a_grant_or_idle_alarm() -> TestResult {
    let RegistryFixture {
        root,
        fleet,
        io,
        config,
        objects,
        registry,
        runtime,
    } = RegistryFixture::open(false).await?;
    // No simultaneous Registry owner: finish the fixture owner before arranging
    // public library lifecycle state, then reopen the actual central runtime.
    drop(runtime);
    objects.close().await?;
    drop(registry);
    drop(objects);
    let database = DatabaseId::new(durable_steps::Uuid::parse_str(DATABASE)?)?;
    registry_setup::import_unheld(root.path(), config, database).await?;
    let (retirement, answer) =
        registry_setup::retire_imported_copy(root.path(), config, database, Epoch::new(2)?).await?;
    assert!(
        matches!(answer, fleet_sync_protocol::RetirementAnswer::Refused(
        fleet_sync_protocol::RetirementRefusal::CopyMismatch { fence, .. }
    ) if fence == Epoch::new(1)?)
    );

    let reopened = ObjectStore::open(root.path()).await?;
    let registry = reopened
        .object(
            ObjectKey::new("registry")?,
            Default::default(),
            Arc::new(CoreHost::for_store(
                io.clone(),
                config.authoritative_receiver,
            )),
        )
        .await?;
    let runtime = ObjectRuntime::registry(
        Arc::new(TokioStorageHost::new(registry.clone())),
        io.clone(),
        config,
        collaboration_central_tokio::DeliveryConfig {
            relay_peer: fleet_sync_protocol::PeerId::try_from(
                "019f0000-0000-7000-8000-000000005240".to_owned(),
            )?,
        },
    )?;
    let proof = async {
        assert_eq!(runtime.initialize().await?, 0);
        assert!(io.calls.lock().map_err(|_| "call observation poisoned")?.is_empty(),
            "startup readiness must not wait for Fleet");
        assert_eq!(*io.wakes.lock().map_err(|_| "wake observation poisoned")?,
            vec![SyncTime::from_unix_millis(1_792_281_600_000)]);

        assert_eq!(runtime.on_alarm().await?, durable_steps::DriveReport::default());

        let stored = fleet.object.transaction(|scope| Box::pin(async move {
            let row = query::<sqlx_turso::Turso>(
                "SELECT lease_projection FROM project_directory WHERE project_id=?",
            ).bind(DATABASE).fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
            row.try_get::<String, _>("lease_projection").map_err(StoreError::backend)
        })).await?;
        let stored: Value = serde_json::from_str(&stored)?;
        let snapshot: fleet_sync_lease::RegistrySnapshot = serde_json::from_value(
            stored.get("snapshot").cloned().ok_or("published snapshot absent")?,
        )?;
        assert_eq!(snapshot.lifecycle,
            fleet_sync_lease::DatabaseLifecycle::Retiring { attempt: retirement });
        assert!(snapshot.grant.is_none());
        assert_eq!(stored.get("holder"), Some(&Value::Null));
        let handled = registry.transaction(|scope| Box::pin(async move {
            let row = query::<sqlx_turso::Turso>(
                "SELECT count(*) AS handled FROM registry_lease_acknowledgements WHERE project_id=?",
            ).bind(DATABASE).fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
            row.try_get::<i64, _>("handled").map_err(StoreError::backend)
        })).await?;
        assert_eq!(handled, 1);
        let calls = io.calls.lock().map_err(|_| "call observation poisoned")?.clone();
        assert_eq!(calls.iter().filter(|call|
            call.get("kind").and_then(Value::as_str) == Some("central.lease-projection")
        ).count(), 1);
        let wakes = io.wakes.lock().map_err(|_| "wake observation poisoned")?.clone();
        assert_eq!(wakes.len(), 1, "successful repair must not arm another alarm");
        // Warm initialization and a later alarm are both idle after the durable acknowledgement.
        assert_eq!(runtime.initialize().await?, 0);
        io.now.fetch_add(120_000, Ordering::SeqCst);
        assert_eq!(runtime.on_alarm().await?, durable_steps::DriveReport::default());
        assert_eq!(*io.calls.lock().map_err(|_| "call observation poisoned")?, calls);
        assert_eq!(*io.wakes.lock().map_err(|_| "wake observation poisoned")?, wakes);
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    reopened.close().await?;
    fleet.objects.close().await?;
    proof
}
