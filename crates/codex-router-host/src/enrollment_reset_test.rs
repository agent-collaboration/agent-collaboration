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
    message_board_storage::FleetStore::open(
        &storage.join(message_board_storage::FleetStore::FILE_NAME),
    )
    .await?
    .close()
    .await;
    let owned_files = [
        "fleet-outbox.sqlite",
        "installation-peer.json",
        "local-history.json",
    ];
    for name in owned_files {
        tokio::fs::write(storage.join(name), name.as_bytes()).await?;
    }
    // Reset now clears central stream progress through this real file owner.
    interaction_storage::InteractionStore::open(storage.join("interaction.sqlite"))
        .await?
        .close()
        .await?;
    drop(record_pending_enrollment_reset(&journal)?);
    let (entered, observed) = oneshot::channel();
    let (_release, released) = oneshot::channel::<()>();
    let resetting_journal = journal.clone();
    let resetting_storage = storage.clone();
    let resetting = tokio::spawn(async move {
        let fleet_store = resetting_storage.join(message_board_storage::FleetStore::FILE_NAME);
        resume_with_barrier(
            &resetting_journal,
            &resetting_storage,
            ResetFleetStore::CreateWhenMissing(&fleet_store),
            async {
                entered
                    .send(())
                    .map_err(|_| io::Error::other("reset observer closed"))?;
                released.await.map_err(io::Error::other)
            },
        )
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
    let cache_store = message_board_storage::FleetStore::open(
        &storage.join(message_board_storage::FleetStore::FILE_NAME),
    )
    .await?;
    let cache = cache_store.cache();
    cache
        .replace_snapshot(&message_board_storage::FleetRecordSnapshot {
            through: message_board::FleetPosition::try_from(7)?,
            records: Vec::new(),
        })
        .await?;
    cache_store.close().await;
    resume_enrollment_reset(&journal, &storage).await?;
    let cache_store = message_board_storage::FleetStore::open(
        &storage.join(message_board_storage::FleetStore::FILE_NAME),
    )
    .await?;
    let cache = cache_store.cache();
    assert_eq!(
        cache.checkpoint().await?,
        message_board::FleetPosition::try_from(7)?
    );
    cache_store.close().await;
    Ok(())
}

/// A collaboration directory laid out as an older Router left it: the Fleet database
/// under its former name, holding a learned checkpoint the reset must clear.
async fn older_root_with_learned_checkpoint()
-> Result<(tempfile::TempDir, std::path::PathBuf), Box<dyn std::error::Error + Send + Sync>> {
    let root = tempfile::tempdir()?;
    let former = crate::router_collaboration_directory(root.path()).join("fleet-cache.sqlite");
    let store = message_board_storage::FleetStore::open(&former).await?;
    store
        .cache()
        .replace_snapshot(&message_board_storage::FleetRecordSnapshot {
            through: message_board::FleetPosition::try_from(7)?,
            records: Vec::new(),
        })
        .await?;
    store.close().await;
    Ok((root, former))
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; fixture errors propagate."
)]
async fn the_update_finishes_a_pending_reset_against_the_store_under_its_former_name() -> TestResult
{
    // Arrange
    let (root, former) = older_root_with_learned_checkpoint().await?;
    let directory = crate::router_collaboration_directory(root.path());
    drop(record_pending_enrollment_reset(&directory)?);

    // Act
    finish_enrollment_reset_for_update(root.path(), &former).await?;

    // Assert: the store the older Router used is the one cleared; no store is created
    // under the current name; the reset is finished.
    let store = message_board_storage::FleetStore::open_existing(&former).await?;
    let checkpoint = store.cache().checkpoint().await;
    store.close().await;
    assert_eq!(checkpoint?, message_board::FleetPosition::try_from(0)?);
    assert!(
        !directory
            .join(message_board_storage::FleetStore::FILE_NAME)
            .exists()
    );
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
async fn the_update_never_creates_the_store_a_pending_reset_would_clear() -> TestResult {
    // Arrange: a pending reset and no Fleet database under either name.
    let root = tempfile::tempdir()?;
    let directory = crate::router_collaboration_directory(root.path());
    drop(record_pending_enrollment_reset(&directory)?);
    let former = directory.join("fleet-cache.sqlite");

    // Act
    let finished = finish_enrollment_reset_for_update(root.path(), &former).await;

    // Assert
    assert!(finished.is_err(), "a missing store must stop the update");
    assert!(!former.exists());
    assert!(
        !directory
            .join(message_board_storage::FleetStore::FILE_NAME)
            .exists()
    );
    assert!(reset_pending_in_directory(&directory)?);
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; fixture errors propagate."
)]
async fn the_update_leaves_a_root_without_an_unfinished_reset_as_it_is() -> TestResult {
    // Arrange: a root whose collaboration directory holds only its Fleet database.
    let (root, former) = older_root_with_learned_checkpoint().await?;
    let directory = crate::router_collaboration_directory(root.path());
    let listing = || -> io::Result<Vec<std::ffi::OsString>> {
        let mut names = std::fs::read_dir(&directory)?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<io::Result<Vec<_>>>()?;
        names.sort();
        Ok(names)
    };
    let before = listing()?;

    // Act
    finish_enrollment_reset_for_update(root.path(), &former).await?;

    // Assert: no journal, no lock file, and the learned checkpoint is kept.
    assert_eq!(listing()?, before);
    let store = message_board_storage::FleetStore::open_existing(&former).await?;
    let checkpoint = store.cache().checkpoint().await;
    store.close().await;
    assert_eq!(checkpoint?, message_board::FleetPosition::try_from(7)?);
    Ok(())
}
