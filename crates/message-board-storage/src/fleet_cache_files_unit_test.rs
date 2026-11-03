use super::*;

#[test]
fn wrong_file_owner_is_rejected_before_permissions_are_changed() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("owned");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    let owner = file.metadata().unwrap().uid();
    assert!(matches!(
        owned_file(&file, owner.wrapping_add(1)),
        Err(RouterLocalStorageError::PrivateFleetCache)
    ));
}
