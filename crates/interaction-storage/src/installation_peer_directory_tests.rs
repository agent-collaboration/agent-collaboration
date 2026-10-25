use super::*;

fn binding(number: u64) -> InstallationPeerBinding {
    InstallationPeerBinding {
        service_id: format!("00000000-0000-4000-8000-{number:012x}")
            .try_into()
            .unwrap(),
        peer_id: format!("019a1234-5678-7000-8000-{number:012x}")
            .try_into()
            .unwrap(),
    }
}

#[tokio::test]
async fn remembered_pair_is_idempotent_and_available_after_cold_reopen_without_a_session() {
    // Arrange: the physical directory has no network or session dependency.
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("interaction.sqlite");
    let store = InteractionStore::open(&path).await.unwrap();
    let original = binding(1);
    assert_eq!(
        store.installation_peer(&original.service_id).await.unwrap(),
        None
    );
    // Act: remember an authenticated result twice, then close the owner completely.
    store.remember_installation_peer(&original).await.unwrap();
    store.remember_installation_peer(&original).await.unwrap();
    store.close().await.unwrap();
    let store = InteractionStore::open(&path).await.unwrap();
    // Assert: the immutable binding survives and no duplicate row was created.
    assert_eq!(
        store.installation_peer(&original.service_id).await.unwrap(),
        Some(original.peer_id)
    );
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM installation_peer_directory")
        .fetch_one(&mut *store.writer.lock().await)
        .await
        .unwrap();
    assert_eq!(count, 1);
    store.close().await.unwrap();
}

#[tokio::test]
async fn either_key_reassignment_refuses_and_rolls_back_the_whole_refresh_page() {
    let directory = tempfile::tempdir().unwrap();
    let store = InteractionStore::open(directory.path().join("interaction.sqlite"))
        .await
        .unwrap();
    let first = binding(1);
    let second = binding(2);
    store
        .remember_installation_peers(&[first.clone(), second.clone()])
        .await
        .unwrap();
    for conflicting in [
        InstallationPeerBinding {
            service_id: first.service_id.clone(),
            peer_id: binding(3).peer_id,
        },
        InstallationPeerBinding {
            service_id: binding(3).service_id,
            peer_id: first.peer_id.clone(),
        },
    ] {
        assert!(matches!(
            store.remember_installation_peer(&conflicting).await,
            Err(StorageError::InstallationPeerConflict)
        ));
    }
    let new_pair = binding(4);
    let conflict = InstallationPeerBinding {
        service_id: first.service_id.clone(),
        peer_id: second.peer_id.clone(),
    };
    assert!(matches!(
        store
            .remember_installation_peers(&[new_pair.clone(), conflict])
            .await,
        Err(StorageError::InstallationPeerConflict)
    ));
    assert_eq!(
        store.installation_peer(&new_pair.service_id).await.unwrap(),
        None
    );
    assert_eq!(
        store.installation_peer(&first.service_id).await.unwrap(),
        Some(first.peer_id)
    );
    assert_eq!(
        store.installation_peer(&second.service_id).await.unwrap(),
        Some(second.peer_id)
    );
    store.close().await.unwrap();
}

#[tokio::test]
async fn a_real_mid_page_database_failure_does_not_leave_a_partial_refresh() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("interaction.sqlite");
    let store = InteractionStore::open(&path).await.unwrap();
    sqlx::raw_sql("CREATE TRIGGER reject_directory_pair BEFORE INSERT ON installation_peer_directory WHEN NEW.service_id='00000000-0000-4000-8000-000000000002' BEGIN SELECT RAISE(ABORT, 'fixture directory insertion failure'); END")
        .execute(&mut *store.writer.lock().await).await.unwrap();
    let result = store
        .remember_installation_peers(&[binding(1), binding(2)])
        .await;
    assert!(
        matches!(result, Err(StorageError::Database(ref error)) if error.to_string().contains("fixture directory insertion failure"))
    );
    sqlx::raw_sql("DROP TRIGGER reject_directory_pair")
        .execute(&mut *store.writer.lock().await)
        .await
        .unwrap();
    store.close().await.unwrap();
    let store = InteractionStore::open(&path).await.unwrap();
    assert_eq!(
        store
            .installation_peer(&binding(1).service_id)
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        store
            .installation_peer(&binding(2).service_id)
            .await
            .unwrap(),
        None
    );
    store.close().await.unwrap();
}

#[tokio::test]
async fn invalid_stored_identities_are_refused_instead_of_overwritten() {
    for corrupt_column in ["peer_id", "service_id"] {
        let directory = tempfile::tempdir().unwrap();
        let store = InteractionStore::open(directory.path().join("interaction.sqlite"))
            .await
            .unwrap();
        let original = binding(1);
        store.remember_installation_peer(&original).await.unwrap();
        if corrupt_column == "peer_id" {
            // The table deliberately has no UUID CHECK; the decoding owner enforces it.
            sqlx::query("UPDATE installation_peer_directory SET peer_id=?")
                .bind("00000000-0000-4000-8000-000000000001")
                .execute(&mut *store.writer.lock().await)
                .await
                .unwrap();
            assert!(matches!(
                store.installation_peer(&original.service_id).await,
                Err(StorageError::InvalidStoredRecord)
            ));
        } else {
            sqlx::query("UPDATE installation_peer_directory SET service_id='not-a-service'")
                .execute(&mut *store.writer.lock().await)
                .await
                .unwrap();
        }
        assert!(matches!(
            store.remember_installation_peer(&original).await,
            Err(StorageError::InvalidStoredRecord)
        ));
        store.close().await.unwrap();
    }
}

#[tokio::test]
async fn exact_preceding_schemas_upgrade_but_partial_directory_migrations_refuse() {
    for omit_retry in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("interaction.sqlite");
        crate::schema_prefix_fixture::create_native_prefix(
            &path,
            if omit_retry {
                202610190002
            } else {
                202610190003
            },
        )
        .await;
        let store = InteractionStore::open(&path).await.unwrap();
        store.remember_installation_peer(&binding(1)).await.unwrap();
        store.close().await.unwrap();
    }
    for statement in [
        "DROP TABLE installation_peer_directory",
        "DELETE FROM _sqlx_migrations WHERE version=202610190004",
        "DROP TABLE local_delivery_retries; DELETE FROM _sqlx_migrations WHERE version=202610190003",
        "ALTER TABLE installation_peer_directory ADD COLUMN unexpected TEXT",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("interaction.sqlite");
        crate::schema_prefix_fixture::create_native_prefix(&path, 202610190004).await;
        let mut connection = sqlx_turso::TursoConnection::connect_with(
            &sqlx_turso::TursoConnectOptions::new().filename(&path),
        )
        .await
        .unwrap();
        sqlx::raw_sql(statement)
            .execute(&mut connection)
            .await
            .unwrap();
        connection.close().await.unwrap();
        let before = tokio::fs::read(&path).await.unwrap();
        assert!(
            matches!(
                InteractionStore::open(&path).await,
                Err(StorageError::InvalidSchema)
            ),
            "accepted partial migration: {statement}"
        );
        assert_eq!(tokio::fs::read(&path).await.unwrap(), before);
    }
}
