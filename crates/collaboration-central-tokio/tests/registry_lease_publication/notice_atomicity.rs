//! A real mark-write error must roll back the already admitted journal job.
use super::*;

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn mark_write_failure_rolls_back_notice_admission_before_delivery_and_retry_recovers()
-> TestResult {
    let fixture = RegistryFixture::open(false).await?;
    let proof = async {
        let owner = OwnerIdentity::from_access(AccessCaller::Owner {email:"fixture@example.test".into()})?;
        let request: EnrollMachine = serde_json::from_value(json!({
            "installation":SERVICE,"peer":"019f0000-0000-7000-8000-000000001f13",
            "label":"notice atomicity writer","access_client_id":"notice-atomicity-writer",
            "credential_hash":"c".repeat(64),"expires_at":"2026-10-20T00:00:00Z"
        }))?;
        let EnrollmentDecision::MachineEnrolled {peer,..} = fixture.runtime.enroll_machine(&owner,request).await? else {
            return Err("notice atomicity fixture enrollment did not activate".into());
        };
        let database = DatabaseId::try_from(DATABASE.to_owned())?;
        registry_setup::register_grant_history(fixture.root.path(), fixture.config, database,
            vec![(Epoch::new(1)?,peer)]).await?;
        fixture.registry.transaction(|scope| Box::pin(async move {
            sqlx_core::raw_sql::raw_sql("CREATE TRIGGER reject_notice_mark BEFORE INSERT ON registry_notice_marks BEGIN SELECT RAISE(ABORT, 'test notice mark refused'); END")
                .execute(scope.executor()).await.map_err(StoreError::backend)?;
            Ok(())
        })).await?;
        fixture.io.calls.lock().map_err(|_| "call observation poisoned")?.clear();
        fixture.io.wakes.lock().map_err(|_| "wake observation poisoned")?.clear();

        let error = match fixture.runtime.on_alarm().await {
            Ok(_) => return Err("mark write unexpectedly committed".into()),
            Err(error) => error,
        };
        assert!(error.to_string().contains("test notice mark refused"), "{error}");
        assert_eq!(counts(&fixture.registry).await?, (0,0));
        assert!(fixture.io.calls.lock().map_err(|_| "call observation poisoned")?.is_empty(),
            "rolled-back notice intent escaped to a remote owner");
        assert_eq!(*fixture.io.wakes.lock().map_err(|_| "wake observation poisoned")?,
            vec![SyncTime::from_unix_millis(1_792_281_605_000)],
            "only the publication repair retry may wake after this failed commit");

        fixture.registry.transaction(|scope| Box::pin(async move {
            query::<sqlx_turso::Turso>("DROP TRIGGER reject_notice_mark")
                .execute(scope.executor()).await.map_err(StoreError::backend)?;
            Ok(())
        })).await?;
        fixture.runtime.on_alarm().await?;
        assert_eq!(counts(&fixture.registry).await?, (1,1),
            "positive retry did not atomically record the mark and actual notice job");
        Ok::<_,Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    fixture.objects.close().await?;
    fixture.fleet.objects.close().await?;
    proof
}

async fn counts(registry: &support::RegistryStore) -> TestResult<(i64, i64)> {
    Ok(registry.transaction(|scope|Box::pin(async move {
        let row = query::<sqlx_turso::Turso>("SELECT (SELECT COUNT(*) FROM registry_notice_marks) AS marks,(SELECT COUNT(*) FROM _fleet_sync_operations WHERE kind='central.registry-notice') AS jobs")
            .fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
        Ok((row.try_get("marks").map_err(StoreError::backend)?,row.try_get("jobs").map_err(StoreError::backend)?))
    })).await?)
}
