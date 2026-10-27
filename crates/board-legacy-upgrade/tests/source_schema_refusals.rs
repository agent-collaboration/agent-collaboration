#![expect(
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
mod support;
use board_legacy_upgrade::UpgradeError;
use support::*;

async fn refuses_unchanged_source_after(sql: &str) -> TestResult {
    let (_directory, path) = fixture_copy("role-history.sqlite")?;
    let source = rusqlite::Connection::open(&path)?;
    source.execute_batch(sql)?;
    source.close().map_err(|(_, error)| error)?;
    let bytes = std::fs::read(&path)?;
    let modified = std::fs::metadata(&path)?.modified()?;

    let result = prepare_path(&path).await;
    let error = match result {
        Ok(prepared) => {
            prepared.close().await;
            return Err("unsupported source schema was accepted".into());
        }
        Err(error) => error,
    };
    let refused = error.downcast_ref::<UpgradeError>().is_some_and(
        |error| matches!(error, UpgradeError::InvalidLegacy { field, .. } if field == "schema"),
    );
    assert!(refused, "expected schema refusal, got {error}");
    assert_eq!(bytes, std::fs::read(&path)?);
    assert_eq!(modified, std::fs::metadata(path)?.modified()?);
    Ok(())
}

#[tokio::test]
async fn changed_schema_is_rejected_without_replacing_data() -> TestResult {
    // Same missing-index corruption as the held legacy BoardStore test.
    refuses_unchanged_source_after("DROP INDEX board_projects_name_unique").await
}

#[tokio::test]
async fn untracked_schema_is_not_adopted() -> TestResult {
    refuses_unchanged_source_after("DROP TABLE _sqlx_migrations").await
}

#[tokio::test]
async fn extra_checkpoint_row_is_rejected() -> TestResult {
    refuses_unchanged_source_after(
        "INSERT INTO activity_checkpoint(singleton,last_sequence,cursor_key) VALUES(2,0,randomblob(32))",
    )
    .await
}

#[tokio::test]
async fn missing_or_changed_migration_identity_is_rejected_before_data_decode() -> TestResult {
    for mutation in [
        "DELETE FROM _sqlx_migrations WHERE version=202610020001",
        "UPDATE _sqlx_migrations SET checksum=randomblob(48) WHERE version=202610020001",
        "UPDATE _sqlx_migrations SET success=0 WHERE version=202610020001",
    ] {
        // A schema refusal must precede decoding even when a data row is invalid.
        let sql = format!("{mutation}; UPDATE board_projects SET name=''");
        refuses_unchanged_source_after(&sql).await?;
    }
    Ok(())
}
