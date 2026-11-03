use super::*;
use std::time::Duration;
use tokio::sync::oneshot;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[cfg(all(debug_assertions, feature = "runtime-test-support"))]
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; fixture errors propagate."
)]
fn debug_reset_barrier_refuses_relative_public_and_aliased_socket_paths() -> TestResult {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir()?;
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
    let socket = root.path().join("barrier.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket)?;
    validate_barrier_socket(&socket)?;
    assert!(validate_barrier_socket(Path::new("barrier.sock")).is_err());
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o755))?;
    assert!(validate_barrier_socket(&socket).is_err());
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
    let alias = root.path().join("alias.sock");
    std::os::unix::fs::symlink(&socket, &alias)?;
    assert!(validate_barrier_socket(&alias).is_err());
    drop(listener);
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; fixture errors propagate."
)]
async fn interrupted_reset_resumes_before_activation_and_preserves_router_owned_content()
-> TestResult {
    // Arrange different journal/storage homes, as supported by a configured Host directory.
    let root = tempfile::tempdir()?;
    let journal = root.path().join("enrollment");
    let storage = root.path().join("configured-collaboration");
    tokio::fs::create_dir_all(storage.join("machine-session")).await?;
    tokio::fs::create_dir_all(storage.join("board-read-cache/generation")).await?;
    tokio::fs::write(
        storage.join("machine-session/state.json"),
        b"old session cursor",
    )
    .await?;
    tokio::fs::write(
        storage.join("machine-session/machine-notice-progress.json"),
        b"old notice cursor",
    )
    .await?;
    tokio::fs::write(
        storage.join("board-read-cache/generation/copy.sqlite"),
        b"old central copy",
    )
    .await?;
    let cache =
        message_board_storage::FleetCache::open(&storage.join("fleet-cache.sqlite")).await?;
    cache.close().await;
    let owned_files = [
        "interaction.sqlite",
        "fleet-outbox.sqlite",
        "installation-peer.json",
        "local-history.json",
    ];
    for name in owned_files {
        tokio::fs::write(storage.join(name), name.as_bytes()).await?;
    }
    drop(record_pending_enrollment_reset(&journal)?);
    let (entered, observed) = oneshot::channel();
    let (_release, released) = oneshot::channel::<()>();
    let resetting_journal = journal.clone();
    let resetting_storage = storage.clone();
    let resetting = tokio::spawn(async move {
        resume_with_barrier(&resetting_journal, &resetting_storage, async {
            entered
                .send(())
                .map_err(|_| io::Error::other("reset observer closed"))?;
            released.await.map_err(io::Error::other)
        })
        .await
    });

    // Act: interrupt after the durable cache-clear checkpoint, before completion.
    tokio::time::timeout(Duration::from_secs(10), observed).await??;
    resetting.abort();
    assert!(resetting.await.is_err_and(|error| error.is_cancelled()));

    // Assert: an interrupted reset still blocks attach and the next start finishes it.
    let reset = load_reset(&journal)?.ok_or("reset journal missing")?;
    assert!(matches!(reset.phase, ResetPhase::RouterStateCleared));
    assert!(reset_pending_in_directory(&journal)?);
    for name in [
        "machine-session/state.json",
        "machine-session/machine-notice-progress.json",
        "fleet-cache.sqlite",
        "board-read-cache",
    ] {
        assert!(
            !storage.join(name).exists(),
            "central-derived state survived: {name}"
        );
    }
    resume_enrollment_reset(&journal, &storage).await?;
    assert!(!reset_pending_in_directory(&journal)?);
    assert!(matches!(
        load_reset(&journal)?.ok_or("reset journal missing")?.phase,
        ResetPhase::Completed
    ));
    for name in owned_files {
        assert_eq!(tokio::fs::read(storage.join(name)).await?, name.as_bytes());
    }
    // Replaying completion cannot clear state learned after the reset.
    tokio::fs::write(storage.join("fleet-cache.sqlite"), b"new central cache").await?;
    resume_enrollment_reset(&journal, &storage).await?;
    assert_eq!(
        tokio::fs::read(storage.join("fleet-cache.sqlite")).await?,
        b"new central cache"
    );
    Ok(())
}
