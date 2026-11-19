use super::{MachineIdentityError, load_or_create_machine_id};
use crate::{MACHINE_LOCAL_DATABASE, MachineLocalStore};
use runtime_primitives::SyncTime;

const FIRST_START: i64 = 1_795_000_000_123;
const OLD_SERVICE_ID: &str = "019a91a3-7000-7000-8000-000000000011";
const OLD_PEER_ID: &str = "019a91a3-7000-7000-8000-000000000012";

#[tokio::test]
async fn first_start_mints_and_restart_reuses_the_id_without_touching_old_files() {
    let root = tempfile::tempdir().unwrap();
    let service_path = root.path().join("service-identity.json");
    let peer_path = root.path().join("installation-peer.json");
    let old_service = format!(r#"{{"version":1,"serviceId":"{OLD_SERVICE_ID}"}}"#);
    let old_peer = format!(r#"{{"version":1,"peerId":"{OLD_PEER_ID}"}}"#);
    std::fs::write(&service_path, &old_service).unwrap();
    std::fs::write(&peer_path, &old_peer).unwrap();

    let first = load_or_create_machine_id(root.path(), SyncTime::from_unix_millis(FIRST_START))
        .await
        .unwrap();
    let restarted =
        load_or_create_machine_id(root.path(), SyncTime::from_unix_millis(FIRST_START + 7_000))
            .await
            .unwrap();
    assert_eq!(first, restarted);
    assert_ne!(first.as_str(), OLD_SERVICE_ID);
    assert_ne!(first.as_str(), OLD_PEER_ID);
    let parsed = uuid::Uuid::parse_str(first.as_str()).unwrap();
    assert_eq!(parsed.get_version_num(), 7);
    let mut store = MachineLocalStore::open(&root.path().join(MACHINE_LOCAL_DATABASE))
        .await
        .unwrap();
    let stamps: (i64, i64) = sqlx::query_as("SELECT created_at, updated_at FROM machine_identity")
        .fetch_one(&mut store.connection)
        .await
        .unwrap();
    assert_eq!(stamps, (FIRST_START, FIRST_START));
    assert_eq!(std::fs::read_to_string(service_path).unwrap(), old_service);
    assert_eq!(std::fs::read_to_string(peer_path).unwrap(), old_peer);
}

#[tokio::test]
async fn concurrent_first_starts_share_one_machine_id() {
    let root = tempfile::tempdir().unwrap();
    let (first, second) = tokio::join!(
        load_or_create_machine_id(root.path(), SyncTime::from_unix_millis(FIRST_START)),
        load_or_create_machine_id(root.path(), SyncTime::from_unix_millis(FIRST_START + 1)),
    );
    assert_eq!(first.unwrap(), second.unwrap());
    assert!(!root.path().join("service-identity.json").exists());
    assert!(!root.path().join("installation-peer.json").exists());
    let mut store = MachineLocalStore::open(&root.path().join(MACHINE_LOCAL_DATABASE))
        .await
        .unwrap();
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM machine_identity")
        .fetch_one(&mut store.connection)
        .await
        .unwrap();
    assert_eq!(rows, 1);
}

#[tokio::test]
async fn a_corrupt_machine_id_fails_reopen_without_reminting() {
    let root = tempfile::tempdir().unwrap();
    let now = SyncTime::from_unix_millis(FIRST_START);
    let path = root.path().join(MACHINE_LOCAL_DATABASE);
    load_or_create_machine_id(root.path(), now).await.unwrap();
    let mut store = MachineLocalStore::open(&path).await.unwrap();
    let corrupt = "80000000-0000-4000-8000-000000000014";
    sqlx::query("UPDATE machine_identity SET machine_id = ?")
        .bind(corrupt)
        .execute(&mut store.connection)
        .await
        .unwrap();
    store.close().await.unwrap();

    let result = load_or_create_machine_id(root.path(), now).await;
    assert!(matches!(
        result,
        Err(MachineIdentityError::InvalidStoredId(_))
    ));
    let mut store = MachineLocalStore::open(&path).await.unwrap();
    let ids: Vec<String> = sqlx::query_scalar("SELECT machine_id FROM machine_identity")
        .fetch_all(&mut store.connection)
        .await
        .unwrap();
    assert_eq!(ids, [corrupt]);
}
