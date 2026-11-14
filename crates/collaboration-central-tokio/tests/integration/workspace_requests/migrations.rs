use super::*;
use collaboration_central_core::{storage_conformance, workspace_migrations};
use collaboration_central_tokio::ObjectStorage;
#[tokio::test]
async fn business_migration_upgrade_failure_rolls_back_both_real_owners_and_all_history_columns() {
    let f = Fixture::new().await;
    f.area(880).await;
    f.host.close().await;
    let cases = [
        (
            f.root
                .path()
                .join("central")
                .join(format!("{}.sqlite", f.caller.organization_id.as_str())),
            workspace_migrations::central_migrator as fn() -> sqlx::migrate::Migrator,
            2,
        ),
        (
            f.root
                .path()
                .join("workspace-stores")
                .join(format!("{}.sqlite", f.workspace)),
            workspace_migrations::workspace_migrator as fn() -> sqlx::migrate::Migrator,
            1,
        ),
    ];
    for (path, migrator, expected_count) in cases {
        let owner = ObjectStorage::new(path.clone(), Arc::new(migrator()));
        let before = owner
            .read(storage_conformance::list_applied_migrations())
            .await
            .unwrap();
        assert_eq!(
            before.columns,
            [
                "version",
                "description",
                "installed_on",
                "success",
                "checksum",
                "execution_time"
            ]
        );
        assert_eq!(before.rows.len(), expected_count);
        owner.close().await;
        let failing = ObjectStorage::new(
            path.clone(),
            Arc::new(workspace_migrations::failing_upgrade(migrator())),
        );
        assert_eq!(
            failing.read(storage_conformance::list_tables()).await,
            Err(central_storage::StoreFailure::Unavailable)
        );
        failing.close().await;
        let reopened = ObjectStorage::new(path, Arc::new(migrator()));
        assert_eq!(
            reopened
                .read(storage_conformance::list_applied_migrations())
                .await
                .unwrap(),
            before
        );
        let tables = reopened
            .read(storage_conformance::list_tables())
            .await
            .unwrap();
        assert!(
            !tables
                .rows
                .iter()
                .any(|row| row[0] == text("rollback_probe"))
        );
        assert!(!tables.rows.iter().any(|row| row[0] == text("operations")
            || row[0] == text("journal")
            || row[0] == text("widgets")));
        reopened.close().await;
    }
}
