//! Real public Fleet-cache storage and stock SQLite backup, with literal checkpoint oracles.
use super::*;
use message_board::FleetPosition;
use message_board_storage::FleetRecordSnapshot;
use std::{os::unix::fs::PermissionsExt, process::Command};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn observes_saved_checkpoint_from_fresh_cut_without_modifying_sealed_b() -> TestResult {
    // Arrange: a real public cache with a nonzero persisted checkpoint.
    let directory = tempfile::tempdir()?;
    let directory = std::fs::canonicalize(directory.path())?;
    let source = directory.join("fleet-cache.sqlite");
    let untouched = directory.join("B.sqlite");
    let scratch = directory.join("A.sqlite");
    let store = FleetStore::open(&source).await?;
    store
        .cache()
        .replace_snapshot(&FleetRecordSnapshot {
            through: FleetPosition::try_from(17)?,
            records: Vec::new(),
        })
        .await?;
    store.close().await;
    for (from, to) in [(&source, &untouched), (&untouched, &scratch)] {
        let input = if from == &untouched {
            let mut uri = url::Url::from_file_path(from).map_err(|()| "invalid fixture path")?;
            uri.set_query(Some("immutable=1"));
            uri.to_string()
        } else {
            from.to_str().ok_or("non-UTF8 fixture path")?.to_owned()
        };
        let status = Command::new("/usr/bin/sqlite3")
            .args(["-readonly", "-nofollow", "-batch", "-bail"])
            .arg(input)
            .arg(format!(".backup '{}'", to.display()))
            .status()?;
        if !status.success() {
            return Err("fixture backup refused".into());
        }
    }
    std::fs::set_permissions(&untouched, std::fs::Permissions::from_mode(0o400))?;
    std::fs::set_permissions(&scratch, std::fs::Permissions::from_mode(0o600))?;
    let sealed_hash = source_hash(&untouched)?;
    // Act / Assert: the actual adapter must read 17, not a default/new database's zero.
    let observed = observe(&scratch, &untouched).await?;
    assert_eq!(observed["checkpoint"], 17);
    assert_eq!(observed["nonzero"], true);
    assert_eq!(source_hash(&untouched)?, sealed_hash);
    assert!(observe(&source, &untouched).await.is_err());
    Ok(())
}
