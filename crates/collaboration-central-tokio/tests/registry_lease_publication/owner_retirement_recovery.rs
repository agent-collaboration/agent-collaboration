//! The committed library outcome precedes directory acknowledgement (decision577).
use super::*;
use fleet_sync_protocol::RetirementAnswer;

fn owner_call(attempt: RequestId, expect_fence: u64) -> TestResult<Vec<u8>> {
    Ok(serde_json::to_vec(&json!({
        "source":{"kind":"registry"},"sourceFace":"edge","authority":{"kind":"owner","value":"fixture@example.test"},
        "destination":{"kind":"registry"},"destinationFace":"registry",
        "identity":{"operation":durable_steps::Uuid::now_v7(),"step":"retire_recovery"},
        "kind":"central.owner-registry","target":{"database":DATABASE},"request":{"kind":"retire","value":{
            "requestId":attempt,"expectFence":expect_fence,"expectAcked":{"epoch":1,"sequence":0}
        }}
    }))?)
}
async fn invoke(
    runtime: &support::RegistryRuntime,
    attempt: RequestId,
    expect_fence: u64,
) -> TestResult<Value> {
    let bytes = runtime
        .on_call(owner_call(attempt, expect_fence)?)
        .await
        .map_err(|error| format!("owner Retire recovery did not answer: {error:?}"))?;
    let reply: Value = serde_json::from_slice(&bytes)?;
    if reply.get("kind") != Some(&json!("retirement")) {
        return Err(format!("Retire recovery returned another operation: {reply}").into());
    }
    reply
        .get("value")
        .cloned()
        .ok_or_else(|| "Retire recovery omitted its outcome".into())
}

#[tokio::test]
async fn owner_retirement_waits_for_directory_ack_after_committed_library_retired_and_store_reopen()
-> TestResult {
    let RegistryFixture {
        root,
        fleet,
        io,
        config,
        objects,
        registry,
        runtime,
    } = RegistryFixture::open(false).await?;
    drop(runtime);
    objects.close().await?;
    drop(registry);
    drop(objects);
    let database = DatabaseId::try_from(DATABASE.to_owned())?;
    registry_setup::import_unheld(root.path(), config, database).await?;
    let (attempt, answer) =
        registry_setup::retire_imported_copy(root.path(), config, database, Epoch::new(1)?).await?;
    let RetirementAnswer::Retired { at } = answer else {
        return Err("recovery setup did not commit the public library retirement".into());
    };
    // This cut is after the real library task and real Copy committed, before
    // any central directory publication. Its admission gate was a setup stand-in;
    // normal Checking/Wiping is covered by the separate owner HTTP scenarios.
    let objects = ObjectStore::open(root.path()).await?;
    let registry = objects
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
        runtime.initialize().await?;
        let expected = json!({"kind":"pending","value":{"requestId":attempt,"phase":"directory"}});
        if invoke(&runtime, attempt, 1).await? != expected {
            return Err("owner outcome escaped before the owed directory publication".into());
        }
        io.lose_publication_answer.store(true, Ordering::SeqCst);
        runtime.on_alarm().await?;
        let stored: String = fleet
            .object
            .transaction(|scope| {
                Box::pin(async move {
                    query::<sqlx_turso::Turso>(
                        "SELECT lease_projection FROM project_directory WHERE project_id=?",
                    )
                    .bind(DATABASE)
                    .fetch_one(scope.executor())
                    .await
                    .map_err(StoreError::backend)?
                    .try_get("lease_projection")
                    .map_err(StoreError::backend)
                })
            })
            .await?;
        let stored: Value = serde_json::from_str(&stored)?;
        let snapshot: fleet_sync_lease::RegistrySnapshot = serde_json::from_value(
            stored
                .get("snapshot")
                .cloned()
                .ok_or("directory lost its actual snapshot")?,
        )?;
        if snapshot.lifecycle != (fleet_sync_lease::DatabaseLifecycle::Retired { at }) {
            return Err("directory fabricated or restamped the public retirement".into());
        }
        let acknowledged: i64 = registry
            .transaction(|scope| {
                Box::pin(async move {
                    query::<sqlx_turso::Turso>(
                        "SELECT COUNT(*) AS count FROM registry_lease_acknowledgements",
                    )
                    .fetch_one(scope.executor())
                    .await
                    .map_err(StoreError::backend)?
                    .try_get("count")
                    .map_err(StoreError::backend)
                })
            })
            .await?;
        if acknowledged != 0 || invoke(&runtime, attempt, 1).await? != expected {
            return Err("lost directory answer was treated as acknowledged".into());
        }
        io.now.fetch_add(6_000, Ordering::SeqCst);
        runtime.on_alarm().await?;
        let expected = json!({"kind":"retired","value":{"at":at}});
        if invoke(&runtime, attempt, 1).await? != expected
            || invoke(&runtime, attempt, 1).await? != expected
        {
            return Err(
                "acknowledged retirement did not replay its original library outcome".into(),
            );
        }
        if invoke(&runtime, attempt, 2).await? != json!({"kind":"conflictingRequest"}) {
            return Err("completed retirement accepted a changed request".into());
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    drop(runtime);
    objects.close().await?;
    drop(registry);
    drop(objects);
    fleet.objects.close().await?;
    proof
}
