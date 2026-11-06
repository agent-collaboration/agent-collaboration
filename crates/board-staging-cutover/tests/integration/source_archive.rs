//! The archive operation moves the whole original directory after real publication.
use crate::{
    cutover_host_fixture::CutoverHostFixture, native_snapshot_fixture::native_fixture,
    publication::preparation, publication_replay::publisher,
};
use board_central_test_support::TestCentral;
use board_staging_cutover::{
    PreparedProjectBundle, SourceArchiveOutcome, archive_confirmed_source,
};
use message_board::ActorOrigin;
use std::os::unix::fs::{MetadataExt, PermissionsExt};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
fn boxed(error: impl std::fmt::Display) -> Box<dyn std::error::Error + Send + Sync> {
    std::io::Error::other(error.to_string()).into()
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A directory rename must preserve every sibling and survive a lost local receipt."
)]
async fn confirmed_publication_archives_the_whole_directory_and_reconciles_a_lost_receipt()
-> TestResult {
    // Arrange a real native source and arbitrary siblings outside its selected database.
    let fixture = CutoverHostFixture::new(None).map_err(boxed)?;
    let source = native_fixture(&fixture).await.map_err(boxed)?;
    let directory = source.path.parent().ok_or("source directory absent")?;
    std::fs::write(
        directory.join("older-epoch.keep"),
        b"preserve every old sibling",
    )?;
    std::fs::create_dir(directory.join("empty-source-directory"))?;
    let original_inode = directory.metadata()?.ino();
    let origin = ActorOrigin {
        installation: String::from(fixture.service.clone()).try_into()?,
        machine: "archive source".to_owned().try_into()?,
    };
    let central_root = tempfile::tempdir()?;
    let central = TestCentral::start_with_peer(central_root.path(), &origin, fixture.peer).await?;
    let result = async {
        let area = tempfile::tempdir()?;
        std::fs::set_permissions(area.path(), std::fs::Permissions::from_mode(0o700))?;
        let input = preparation(&origin, &source, central.base_url())?;
        let mut offline = fixture.acquire().map_err(boxed)?;
        let bundle = PreparedProjectBundle::prepare(
            &mut offline,
            source.selected.clone(),
            input,
            area.path(),
        )
        .await?;
        let owner = publisher(central.base_url(), area.path())?;
        let confirmed = owner.publish(&mut offline, &bundle).await?;
        // Act after the real Central status became Complete, under the same Host lock.
        let archived = archive_confirmed_source(&mut offline, &bundle, confirmed)?;
        // Assert the original physical directory, all siblings, and native file travel together.
        let backup = &bundle.manifest().backup_directory;
        assert_eq!(
            archived,
            SourceArchiveOutcome::Archived {
                directory: backup.clone()
            }
        );
        assert!(!directory.exists());
        assert_eq!(backup.metadata()?.ino(), original_inode);
        assert_eq!(
            std::fs::read(backup.join("older-epoch.keep"))?,
            b"preserve every old sibling"
        );
        assert!(backup.join("empty-source-directory").is_dir());
        assert!(backup.join("e1.sqlite").is_file());
        assert!(bundle.manifest().source_fingerprint.matches(backup)?);
        let bundle_path = bundle.directory().to_owned();
        drop(archived);
        drop(bundle);
        drop(owner);
        drop(offline);
        // A lost local result is resolved by actual paths, never a persisted success flag.
        let reopened = PreparedProjectBundle::open(&bundle_path)?;
        let mut offline = fixture.acquire().map_err(boxed)?;
        let confirmed = publisher(central.base_url(), area.path())?
            .publish(&mut offline, &reopened)
            .await?;
        assert_eq!(
            archive_confirmed_source(&mut offline, &reopened, confirmed)?,
            SourceArchiveOutcome::PreviouslyArchived {
                directory: reopened.manifest().backup_directory.clone()
            }
        );
        assert_eq!(
            reopened.manifest().backup_directory.metadata()?.ino(),
            original_inode
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let stopped = central.shutdown().await;
    result?;
    stopped?;
    Ok(())
}

#[derive(Clone, Copy)]
enum RefusalCase {
    SourceChanged,
    DestinationCollision,
    ForeignConfirmation,
    ChangedBackup,
}

async fn refused_archive(case: RefusalCase) -> TestResult {
    let fixture = CutoverHostFixture::new(None).map_err(boxed)?;
    let source = native_fixture(&fixture).await.map_err(boxed)?;
    let origin = ActorOrigin {
        installation: String::from(fixture.service.clone()).try_into()?,
        machine: "archive refusal source".to_owned().try_into()?,
    };
    let central_root = tempfile::tempdir()?;
    let central = TestCentral::start_with_peer(central_root.path(), &origin, fixture.peer).await?;
    let result = async {
        let area = tempfile::tempdir()?;
        std::fs::set_permissions(area.path(), std::fs::Permissions::from_mode(0o700))?;
        let input = preparation(&origin, &source, central.base_url())?;
        let mut offline = fixture.acquire().map_err(boxed)?;
        let bundle = PreparedProjectBundle::prepare(
            &mut offline,
            source.selected.clone(),
            input.clone(),
            area.path(),
        )
        .await?;
        let source_dir = &bundle.manifest().source_directory;
        let backup = &bundle.manifest().backup_directory;
        let original_inode = source_dir.metadata()?.ino();
        let owner = publisher(central.base_url(), area.path())?;
        let confirmed = owner.publish(&mut offline, &bundle).await?;
        match case {
            RefusalCase::SourceChanged => {
                // A sibling added after preparation is part of custody too.
                std::fs::write(
                    source_dir.join("new-unsnapshotted.keep"),
                    b"must remain at source",
                )?;
                let outcome = archive_confirmed_source(&mut offline, &bundle, confirmed);
                assert!(outcome.is_err());
                assert_eq!(source_dir.metadata()?.ino(), original_inode);
                assert_eq!(
                    std::fs::read(source_dir.join("new-unsnapshotted.keep"))?,
                    b"must remain at source"
                );
                assert!(!backup.exists());
            }
            RefusalCase::DestinationCollision => {
                use std::os::unix::fs::DirBuilderExt;
                std::fs::DirBuilder::new()
                    .recursive(true)
                    .mode(0o700)
                    .create(backup)?;
                let collision_inode = backup.metadata()?.ino();
                let outcome = archive_confirmed_source(&mut offline, &bundle, confirmed);
                assert!(outcome.is_err());
                assert_eq!(source_dir.metadata()?.ino(), original_inode);
                assert_eq!(backup.metadata()?.ino(), collision_inode);
                assert_eq!(std::fs::read_dir(backup)?.count(), 0);
            }
            RefusalCase::ForeignConfirmation => {
                let mut other = input;
                other.run_id = uuid::Uuid::now_v7();
                let other_bundle = PreparedProjectBundle::prepare(
                    &mut offline,
                    source.selected.clone(),
                    other,
                    area.path(),
                )
                .await?;
                let outcome = archive_confirmed_source(&mut offline, &other_bundle, confirmed);
                assert!(outcome.is_err());
                assert_eq!(source_dir.metadata()?.ino(), original_inode);
                assert!(!other_bundle.manifest().backup_directory.exists());
            }
            RefusalCase::ChangedBackup => {
                archive_confirmed_source(&mut offline, &bundle, confirmed)?;
                let confirmed = owner.publish(&mut offline, &bundle).await?;
                // Keep this archive-fence proof after the new publication preflight.
                std::fs::write(backup.join("changed-after-move.keep"), b"unverified backup")?;
                let outcome = archive_confirmed_source(&mut offline, &bundle, confirmed);
                assert!(outcome.is_err());
                assert!(!source_dir.exists());
                assert_eq!(backup.metadata()?.ino(), original_inode);
                assert_eq!(
                    std::fs::read(backup.join("changed-after-move.keep"))?,
                    b"unverified backup"
                );
            }
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let stopped = central.shutdown().await;
    result?;
    stopped?;
    Ok(())
}

#[tokio::test]
async fn a_changed_source_refuses_archive_without_moving_its_files() -> TestResult {
    refused_archive(RefusalCase::SourceChanged).await
}
#[tokio::test]
async fn an_existing_empty_backup_is_never_overwritten() -> TestResult {
    refused_archive(RefusalCase::DestinationCollision).await
}
#[tokio::test]
async fn another_run_confirmation_cannot_archive_this_source() -> TestResult {
    refused_archive(RefusalCase::ForeignConfirmation).await
}
#[tokio::test]
async fn a_modified_backup_cannot_be_reported_as_a_verified_previous_archive() -> TestResult {
    refused_archive(RefusalCase::ChangedBackup).await
}
