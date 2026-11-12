use super::*;
use std::time::Duration;
use tokio::sync::oneshot;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

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
        serde_json::to_vec(&fleet_sync_session::ClientSessionState {
            session_id: fleet_sync_protocol::SessionId::new(uuid::Uuid::now_v7())?,
            notice_cursor: fleet_sync_protocol::NoticePosition::new(17),
        })?,
    )
    .await?;
    tokio::fs::write(
        storage.join("machine-session/machine-notice-progress.json"),
        br#"{"handledThrough":17}"#,
    )
    .await?;
    tokio::fs::write(
        storage.join("board-read-cache/generation/copy.sqlite"),
        b"old central copy",
    )
    .await?;
    let owned_files = [
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
    tokio::fs::write(storage.join("machine-session/state.json"), b"learned").await?;
    resume_enrollment_reset(&journal, &storage).await?;
    assert_eq!(
        tokio::fs::read(storage.join("machine-session/state.json")).await?,
        b"learned"
    );
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; fixture errors propagate."
)]
async fn the_update_finishes_a_pending_reset() -> TestResult {
    // Arrange: a pending reset beside central-derived session state.
    let root = tempfile::tempdir()?;
    let directory = crate::router_collaboration_directory(root.path());
    tokio::fs::create_dir_all(directory.join("machine-session")).await?;
    tokio::fs::write(directory.join("machine-session/state.json"), b"old").await?;
    drop(record_pending_enrollment_reset(&directory)?);

    // Act
    finish_enrollment_reset_for_update(root.path()).await?;

    // Assert: the central-derived state is cleared and the reset is finished.
    assert!(!directory.join("machine-session/state.json").exists());
    assert_eq!(
        completed_enrollment_reset_journal(root.path())?,
        Some(directory.join(JOURNAL_FILE))
    );
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; fixture errors propagate."
)]
async fn the_update_leaves_a_root_without_an_unfinished_reset_as_it_is() -> TestResult {
    // Arrange: a root whose collaboration directory holds only its session state.
    let root = tempfile::tempdir()?;
    let directory = crate::router_collaboration_directory(root.path());
    tokio::fs::create_dir_all(directory.join("machine-session")).await?;
    tokio::fs::write(directory.join("machine-session/state.json"), b"learned").await?;
    let listing = || -> io::Result<Vec<std::ffi::OsString>> {
        let mut names = std::fs::read_dir(&directory)?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<io::Result<Vec<_>>>()?;
        names.sort();
        Ok(names)
    };
    let before = listing()?;

    // Act
    finish_enrollment_reset_for_update(root.path()).await?;

    // Assert: no journal, no lock file, and the learned state is kept.
    assert_eq!(listing()?, before);
    assert_eq!(
        tokio::fs::read(directory.join("machine-session/state.json")).await?,
        b"learned"
    );
    Ok(())
}
