use super::login_storage_tests::open_fleet_cache_with_fixed_sources;
use std::os::unix::fs::PermissionsExt;

#[tokio::test]
async fn fleet_cache_permissions_cover_the_actual_native_file_set() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("fleet-cache.sqlite");
    let cache = open_fleet_cache_with_fixed_sources(&path).await.unwrap();
    let entries = std::fs::read_dir(directory.path())
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let names = entries
        .iter()
        .map(|entry| entry.file_name())
        .collect::<Vec<_>>();
    assert!(path.is_file(), "main database is missing: {names:?}");
    assert!(
        entries.len() >= 2,
        "expected the live native WAL file set: {names:?}"
    );
    for entry in entries {
        let metadata = entry.metadata().unwrap();
        assert!(metadata.is_file(), "unexpected native output: {names:?}");
        assert_eq!(
            metadata.permissions().mode() & 0o777,
            0o600,
            "native file {:?}; actual set {names:?}",
            entry.file_name()
        );
    }
    cache.close().await;
}

#[tokio::test]
async fn fleet_cache_permissions_tighten_a_populated_0644_cache_without_rewriting_rows() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("fleet-cache.sqlite");
    let cache = open_fleet_cache_with_fixed_sources(&path).await.unwrap();
    sqlx::query("UPDATE fleet_cache_checkpoint SET position=73 WHERE singleton=1")
        .execute(&cache.database.pool)
        .await
        .unwrap();
    cache.close().await;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    let reopened = open_fleet_cache_with_fixed_sources(&path).await.unwrap();
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let position: i64 =
        sqlx::query_scalar("SELECT position FROM fleet_cache_checkpoint WHERE singleton=1")
            .fetch_one(&reopened.database.pool)
            .await
            .unwrap();
    assert_eq!(position, 73);
    reopened.close().await;
}

#[tokio::test]
async fn fleet_cache_permissions_refuse_a_symlink_without_tightening_its_target() {
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("unrelated");
    std::fs::write(&target, b"unrelated bytes").unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644)).unwrap();
    let path = directory.path().join("fleet-cache.sqlite");
    std::os::unix::fs::symlink(&target, &path).unwrap();
    let result = open_fleet_cache_with_fixed_sources(&path).await;
    assert!(matches!(
        result,
        Err(crate::RouterLocalStorageError::PrivateFleetCache)
    ));
    assert_eq!(std::fs::read(&target).unwrap(), b"unrelated bytes");
    assert_eq!(
        std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o644
    );
}
