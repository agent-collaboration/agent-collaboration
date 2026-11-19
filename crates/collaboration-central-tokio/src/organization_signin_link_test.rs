use super::OrganizationSignInLink;
use std::{fs, io, os::unix::fs::MetadataExt, os::unix::net::UnixListener};

#[tokio::test]
async fn stale_cleanup_preserves_a_file_a_live_listener_and_a_foreign_owner() {
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("signin.sock");
    let owner = fs::metadata(directory.path()).unwrap().uid();

    fs::write(&socket, b"keep").unwrap();
    assert!(OrganizationSignInLink::clear_stale_owned_socket(&socket, owner).is_err());
    assert_eq!(fs::read(&socket).unwrap(), b"keep");
    fs::remove_file(&socket).unwrap();

    let live = UnixListener::bind(&socket).unwrap();
    assert_eq!(
        OrganizationSignInLink::clear_stale_owned_socket(&socket, owner)
            .unwrap_err()
            .kind(),
        io::ErrorKind::AddrInUse
    );
    drop(live);
    assert!(OrganizationSignInLink::clear_stale_owned_socket(&socket, owner ^ 1).is_err());
    assert!(socket.exists());

    OrganizationSignInLink::clear_stale_owned_socket(&socket, owner).unwrap();
    assert!(!socket.exists());
}
