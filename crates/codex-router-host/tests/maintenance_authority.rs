//! Maintenance holds the shipped Host lock while leaving socket publication to HostInstance.
#![expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions must fail the proof."
)]
use codex_router_host::{
    HostCoordinationPaths, HostInstance, HostInstanceLock, InstanceAcquireError,
};
use std::os::unix::fs::{MetadataExt, PermissionsExt};

#[tokio::test]
async fn maintenance_authority_blocks_host_start_and_preserves_the_socket_artifact()
-> Result<(), Box<dyn std::error::Error>> {
    // Arrange: pin a stale socket so both its path and inode remain observable.
    let directory = tempfile::tempdir()?;
    let paths = HostCoordinationPaths::new(
        directory.path().join("host.sock"),
        directory.path().join("host.lock"),
    );
    let stale = std::os::unix::net::UnixListener::bind(paths.operator_socket())?;
    let inode = std::fs::metadata(paths.operator_socket())?.ino();

    // Act: acquire only maintenance authority, with no socket removal or bind.
    let authority = HostInstanceLock::acquire(&paths)?;

    // Assert: Host cannot start until the upgrade releases its guard.
    assert_eq!(std::fs::metadata(paths.operator_socket())?.ino(), inode);
    assert_eq!(
        std::fs::metadata(paths.instance_lock())?
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(matches!(
        HostInstance::acquire(paths.clone()),
        Err(InstanceAcquireError::AlreadyRunning)
    ));
    assert_eq!(std::fs::metadata(paths.operator_socket())?.ino(), inode);
    drop(authority);
    drop(stale);
    let host = HostInstance::acquire(paths)?;
    drop(host);
    Ok(())
}
