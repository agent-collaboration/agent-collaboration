//! Decision577/580: version reports come from real transactional history.
use super::*;
use serde_json::{Value, json};

const DATABASE: &str = "019f0000-0000-7000-8000-000000000603";

fn runtime(fixture: &Fixture) -> Result<ObjectRuntime<CopyKind, NativeHost, ProbeIo>, TestError> {
    Ok(ObjectRuntime::copy(
        fixture.host.clone(),
        ProbeIo,
        ReceiverId::try_from("019f0000-0000-7000-8000-000000000604".to_owned())?,
        DatabaseId::try_from(DATABASE.to_owned())?,
        std::time::Duration::from_secs(2),
    ))
}

async fn migrate(fixture: &Fixture) -> Result<Value, TestError> {
    let bytes = runtime(fixture)?
        .on_call(serde_json::to_vec(&json!({
            "source":{"kind":"registry"},"sourceFace":"edge",
            "authority":{"kind":"owner","value":"fixture@example.test"},
            "destination":{"kind":"copy","value":DATABASE},"destinationFace":"copy",
            "identity":{"operation":durable_steps::Uuid::now_v7(),"step":"migrate"},
            "kind":"central.copy-operations","target":{"database":DATABASE},
            "request":{"kind":"migrate"}
        }))?)
        .await
        .map_err(|error| format!("owner Migrate did not answer: {error:?}"))?;
    let answer: Value = serde_json::from_slice(&bytes)?;
    if answer.get("kind") != Some(&json!("migration")) {
        return Err(format!("owner Migrate returned another operation's answer: {answer}").into());
    }
    answer
        .get("value")
        .cloned()
        .ok_or_else(|| "missing migration result".into())
}

async fn version_one(fixture: &Fixture) -> Result<(), TestError> {
    fixture.object.transaction(|scope| Box::pin(async move {
        let migration = fleet_sync_receiver::RECEIVER_MIGRATOR.iter()
            .find(|migration| migration.version == 1)
            .ok_or_else(|| StoreError::backend("public receiver version1 absent"))?;
        scope.executor().execute(migration.sql.as_ref()).await.map_err(StoreError::backend)?;
        scope.executor().execute(include_str!("../../../collaboration-central-core/core_migrations/0000_migration_history.sql"))
            .await.map_err(StoreError::backend)?;
        let checksum = serde_json::to_string(migration.checksum.as_ref()).map_err(StoreError::backend)?;
        query::<sqlx_turso::Turso>("INSERT INTO central_schema_migrations(row_id,corpus,version,checksum) VALUES (?,'fleet-sync-receiver','1',?)")
            .bind(durable_steps::Uuid::now_v7().to_string()).bind(checksum).execute(scope.executor()).await.map_err(StoreError::backend)?;
        scope.executor().execute("CREATE TABLE retained_project_rows(id TEXT NOT NULL PRIMARY KEY,value TEXT NOT NULL); INSERT INTO retained_project_rows VALUES ('row','retained')")
            .await.map_err(StoreError::backend)?;
        Ok(())
    })).await?;
    Ok(())
}

#[tokio::test]
async fn migrate_reports_absent_and_recorded_start_versions_then_current_without_reapplying()
-> Result<(), TestError> {
    for from in [None, Some(1_u64)] {
        // Arrange: a real empty store or the literal public version1 corpus.
        let root = tempfile::tempdir()?;
        let fixture = Fixture::open(root.path()).await?;
        let proof = async {
            if from.is_some() {
                version_one(&fixture).await?;
            }
            // Act: enter at the production owner envelope, before its prelude.
            let first = migrate(&fixture).await?;
            // Assert: decision577's literal from/to, then the idempotent result.
            if first != json!({"kind":"migrated","corpus":"fleet-sync-receiver","from":from,"to":3})
            {
                return Err(format!("Migrate lost its prelude start version: {first}").into());
            }
            let second = migrate(&fixture).await?;
            if second != json!({"kind":"current","corpus":"fleet-sync-receiver","version":3}) {
                return Err(format!("Migrate was not idempotent: {second}").into());
            }
            if from.is_some() {
                let value: String = fixture
                    .object
                    .transaction(|scope| {
                        Box::pin(async move {
                            query::<sqlx_turso::Turso>(
                                "SELECT value FROM retained_project_rows WHERE id='row'",
                            )
                            .fetch_one(scope.executor())
                            .await
                            .map_err(StoreError::backend)?
                            .try_get("value")
                            .map_err(StoreError::backend)
                        })
                    })
                    .await?;
                if value != "retained" {
                    return Err("Migrate changed project data".into());
                }
            }
            Ok::<_, TestError>(())
        }
        .await;
        fixture.objects.close().await?;
        proof?;
    }
    Ok(())
}

#[tokio::test]
async fn migrate_reports_real_receiver_rollback_then_retries_from_the_retained_version()
-> Result<(), TestError> {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let proof = async {
        version_one(&fixture).await?;
        fixture.object.transaction(|scope| Box::pin(async move {
            scope.executor().execute("CREATE TRIGGER receiver_migrate_fault BEFORE INSERT ON central_schema_migrations WHEN NEW.corpus='fleet-sync-receiver' AND NEW.version='3' BEGIN SELECT RAISE(ABORT,'migrate version3 fault'); END")
                .await.map_err(StoreError::backend)?;
            Ok(())
        })).await?;
        let failed = migrate(&fixture).await?;
        if failed.get("kind") != Some(&json!("failed")) || failed.get("corpus") != Some(&json!("fleet-sync-receiver"))
            || failed.get("version") != Some(&json!(3))
            || !failed.get("reason").and_then(Value::as_str).is_some_and(|reason| reason.contains("migrate version3 fault")) {
            return Err(format!("Migrate hid the failed corpus/version/cause: {failed}").into());
        }
        let history: Vec<String> = fixture.object.transaction(|scope| Box::pin(async move {
            query::<sqlx_turso::Turso>("SELECT version FROM central_schema_migrations WHERE corpus='fleet-sync-receiver' ORDER BY version")
                .fetch_all(scope.executor()).await.map_err(StoreError::backend)?
                .into_iter().map(|row| row.try_get("version").map_err(StoreError::backend)).collect()
        })).await?;
        if history != ["1"] { return Err(format!("failed Migrate committed partial history: {history:?}").into()); }
        fixture.object.transaction(|scope| Box::pin(async move {
            scope.executor().execute("DROP TRIGGER receiver_migrate_fault").await.map_err(StoreError::backend)?;
            Ok(())
        })).await?;
        if migrate(&fixture).await? != json!({"kind":"migrated","corpus":"fleet-sync-receiver","from":1,"to":3}) {
            return Err("recovered Migrate did not resume from version1".into());
        }
        let failures: i64 = fixture.object.transaction(|scope| Box::pin(async move {
            query::<sqlx_turso::Turso>("SELECT COUNT(*) AS count FROM central_schema_migration_failures")
                .fetch_one(scope.executor()).await.map_err(StoreError::backend)?
                .try_get("count").map_err(StoreError::backend)
        })).await?;
        if failures != 0 { return Err("successful Migrate retained its failed-corpus diagnostic".into()); }
        Ok::<_, TestError>(())
    }.await;
    fixture.objects.close().await?;
    proof
}

#[tokio::test]
async fn migrate_refuses_populated_receiver_without_history_instead_of_adopting_its_shape()
-> Result<(), TestError> {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let proof = async {
        let bootstrap = ObjectBootstrap::new(fixture.host.clone(), ProbeIo.now());
        bootstrap.initialize(EngineSchema::Copy).await?;
        bootstrap.execute(BootstrapCommand::Probe { schema: BootstrapProbe::Copy }).await?;
        fixture.object.transaction(|scope| Box::pin(async move {
            scope.executor().execute("CREATE TABLE retained_project_rows(id TEXT NOT NULL PRIMARY KEY,value TEXT NOT NULL); INSERT INTO retained_project_rows VALUES ('row','retained'); DELETE FROM central_schema_migrations")
                .await.map_err(StoreError::backend)?;
            Ok(())
        })).await?;
        let before = serde_json::to_value(runtime(&fixture)?.read_copy_status().await.map_err(|error|format!("status setup: {error:?}"))?)?;
        let failed = migrate(&fixture).await?;
        if failed.get("kind") != Some(&json!("failed")) || failed.get("corpus") != Some(&json!("fleet-sync-receiver"))
            || failed.get("version") != Some(&Value::Null) || !failed.get("reason").and_then(Value::as_str).is_some_and(|reason| !reason.is_empty()) {
            return Err(format!("unversioned receiver was adopted or assigned a synthetic version: {failed}").into());
        }
        let after = serde_json::to_value(runtime(&fixture)?.read_copy_status().await.map_err(|error|format!("retained status: {error:?}"))?)?;
        if after != before { return Err("refused adoption changed the actual receiver state".into()); }
        let (history, retained): (i64, String) = fixture.object.transaction(|scope| Box::pin(async move {
            let history = query::<sqlx_turso::Turso>("SELECT COUNT(*) AS count FROM central_schema_migrations")
                .fetch_one(scope.executor()).await.map_err(StoreError::backend)?
                .try_get("count").map_err(StoreError::backend)?;
            let retained = query::<sqlx_turso::Turso>("SELECT value FROM retained_project_rows WHERE id='row'")
                .fetch_one(scope.executor()).await.map_err(StoreError::backend)?
                .try_get("value").map_err(StoreError::backend)?;
            Ok((history, retained))
        })).await?;
        if history != 0 || retained != "retained" { return Err("failed adoption wrote history or changed project data".into()); }
        Ok::<_, TestError>(())
    }.await;
    fixture.objects.close().await?;
    proof
}

#[tokio::test]
async fn migrate_retired_receipt_never_recreates_metadata_or_changes_its_timestamp()
-> Result<(), TestError> {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let proof = async {
        let bootstrap = ObjectBootstrap::new(fixture.host.clone(), ProbeIo.now());
        bootstrap.initialize(EngineSchema::Copy).await?;
        bootstrap.execute(BootstrapCommand::Probe { schema: BootstrapProbe::Copy }).await?;
        let CopyStatusAnswer::Status(status) = runtime(&fixture)?.read_copy_status().await.map_err(|error|format!("retirement setup: {error:?}"))? else {
            return Err("retirement setup has no actual Copy".into());
        };
        let request = fleet_sync_protocol::RetireCopy { database: status.database, expect_fence: status.fence, expect_acked: status.acked };
        fixture.object.transaction(|scope| Box::pin(async move {
            if fleet_sync_receiver::retire_copy(scope, &request, ProbeIo.now()).await.map_err(fleet_sync_receiver::receiver_rollback)?
                != fleet_sync_protocol::RetireCopyAnswer::Retired { return Err(StoreError::backend("real receiver retirement refused")); }
            scope.executor().execute("DROP TABLE central_schema_migrations; DROP TABLE central_schema_migration_failures")
                .await.map_err(StoreError::backend)?;
            Ok(())
        })).await?;
        let before = fixture.object.transaction(|scope| Box::pin(async move { Ok::<_, StoreError>(fleet_sync_receiver::inspect_copy(scope).await) })).await?;
        let tables = fixture.tables().await?;
        for _ in 0..2 {
            if migrate(&fixture).await? != json!({"kind":"retired"}) { return Err("Migrate did not return the closed Retired item".into()); }
        }
        let after = fixture.object.transaction(|scope| Box::pin(async move { Ok::<_, StoreError>(fleet_sync_receiver::inspect_copy(scope).await) })).await?;
        if after != before || fixture.tables().await? != tables { return Err("Migrate rewrote retirement or recreated its metadata".into()); }
        Ok::<_, TestError>(())
    }.await;
    fixture.objects.close().await?;
    proof
}

#[tokio::test]
async fn migrate_names_hook_failure_separately_and_recovers_without_partial_receiver_history()
-> Result<(), TestError> {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let proof = async {
        // Central owns this hook DDL. A real collision runs after receiver migrations.
        fixture.object.transaction(|scope| Box::pin(async move {
            scope.executor().execute("CREATE TABLE _copy_summary(fixture TEXT)").await.map_err(StoreError::backend)?;
            Ok(())
        })).await?;
        let failed = migrate(&fixture).await?;
        if failed.get("kind") != Some(&json!("failed")) || failed.get("corpus") != Some(&json!("central-copy-summary"))
            || failed.get("version") != Some(&json!(1))
            || !failed.get("reason").and_then(Value::as_str).is_some_and(|reason|reason.contains("_copy_summary")) {
            return Err(format!("hook failure was blamed on the receiver or lost its own version: {failed}").into());
        }
        let history_tables: i64 = fixture.object.transaction(|scope| Box::pin(async move {
            query::<sqlx_turso::Turso>("SELECT COUNT(*) AS count FROM sqlite_schema WHERE type='table' AND name='central_schema_migrations'")
                .fetch_one(scope.executor()).await.map_err(StoreError::backend)?
                .try_get("count").map_err(StoreError::backend)
        })).await?;
        if history_tables != 0 {return Err("hook failure committed receiver migration history".into());}
        fixture.object.transaction(|scope| Box::pin(async move {
            scope.executor().execute("DROP TABLE _copy_summary").await.map_err(StoreError::backend)?;
            Ok(())
        })).await?;
        if migrate(&fixture).await? != json!({"kind":"migrated","corpus":"fleet-sync-receiver","from":null,"to":3}) {
            return Err("hook recovery did not start from rolled-back history absence".into());
        }
        Ok::<_,TestError>(())
    }.await;
    fixture.objects.close().await?;
    proof
}

#[tokio::test]
async fn migrate_reports_corrupt_history_without_inventing_version_zero() -> Result<(), TestError> {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let proof = async {
        version_one(&fixture).await?;
        fixture
            .object
            .transaction(|scope| {
                Box::pin(async move {
                    scope
                        .executor()
                        .execute("UPDATE central_schema_migrations SET version='unreadable'")
                        .await
                        .map_err(StoreError::backend)?;
                    Ok(())
                })
            })
            .await?;
        let before = fixture.tables().await?;
        let failed = migrate(&fixture).await?;
        if failed.get("kind") != Some(&json!("failed"))
            || failed.get("corpus") != Some(&json!("fleet-sync-receiver"))
            || failed.get("version") != Some(&Value::Null)
            || !failed
                .get("reason")
                .and_then(Value::as_str)
                .is_some_and(|reason| reason.contains("invalid version"))
        {
            return Err(
                format!("corrupt history invented a version or hid its cause: {failed}").into(),
            );
        }
        if fixture.tables().await? != before {
            return Err("corrupt history allowed a partial migration".into());
        }
        Ok::<_, TestError>(())
    }
    .await;
    fixture.objects.close().await?;
    proof
}
