mod support;
use message_board::*;
use support::*;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn retained_post_without_creation_activity_refuses_without_changing_source() -> TestResult {
    // Arrange: activity 8 is an unreferenced reply creation in the shipped fixture.
    let (_directory, path) = fixture_copy("legacy-complete.sqlite")?;
    let connection = rusqlite::Connection::open(&path)?;
    assert_eq!(
        connection.execute("DELETE FROM board_activity WHERE activity_sequence=8", [])?,
        1
    );
    connection.close().map_err(|(_, error)| error)?;
    let bytes = std::fs::read(&path)?;
    let modified = path.metadata()?.modified()?;
    // Act: exercise the actual reader, transform and batch preparation.
    let result = prepare_path(&path).await;
    // Assert: omitting retained source history must not produce an accepted shortened batch.
    let error = match result {
        Ok(prepared) => {
            prepared.close().await;
            return Err("missing source post creation was accepted".into());
        }
        Err(error) => error,
    };
    assert!(
        matches!(error.downcast_ref::<board_legacy_upgrade::UpgradeError>(),
        Some(board_legacy_upgrade::UpgradeError::InvalidLegacy { field, .. }) if field=="post.coverage"),
        "{error}"
    );
    assert_eq!(std::fs::read(&path)?, bytes);
    assert_eq!(path.metadata()?.modified()?, modified);
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn supplied_legacy_fixture_produces_all_three_project_imports()
-> Result<(), Box<dyn std::error::Error>> {
    // Arrange: retained X2 input built through the shipped ffced1c1 BoardStore.
    // Act: the product importer reads and prepares the legacy store.
    let prepared = prepare("legacy-complete.sqlite").await?;
    // Assert: all legacy topics survive as projects; an empty importer is wrong.
    assert_eq!(prepared.projects.len(), 3);
    assert_eq!(
        prepared
            .report
            .projects
            .iter()
            .map(|project| project.posts)
            .sum::<u64>(),
        11
    );
    prepared.close().await;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn imported_feed_keeps_dense_positions_unknown_times_and_root_join_cutoffs() -> TestResult {
    let prepared = prepare("legacy-complete.sqlite").await?;
    let report = prepared
        .report
        .projects
        .iter()
        .find(|project| project.project.as_str().ends_with("0015"))
        .ok_or("project missing")?;
    assert_eq!(
        report
            .old_to_new
            .get(&1)
            .ok_or("first post mapping missing")?
            .position
            .get(),
        3
    );
    assert_eq!(report.old_to_new.get(&1), report.old_to_new.get(&2));
    for project in &prepared.projects {
        let mut snapshot = project.store.snapshot().await?;
        let FeedRead::Entries {
            entries,
            high_water,
            ..
        } = snapshot
            .feed_after(
                FeedCursor {
                    epoch: EpochNumber::INITIAL,
                    position: FeedPosition::ZERO,
                },
                1000,
                false,
            )
            .await?
        else {
            return Err("unexpected superseded import epoch".into());
        };
        assert_eq!(entries.len() as u64, high_water.position.get());
        for (offset, entry) in entries.iter().enumerate() {
            assert_eq!(entry.position.get(), offset as u64 + 1);
            assert_eq!(entry.epoch, EpochNumber::INITIAL);
            assert_eq!(entry.time, RecordTime::LegacyUnknown);
        }
        assert!(entries.iter().all(|entry| !matches!(
            entry.record.kind,
            RecordKind::Workstream | RecordKind::Adr | RecordKind::WorkTrailRow | RecordKind::Fact
        )));
        snapshot.finish().await?;
    }
    prepared.close().await;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn unchanged_input_rebuilds_identical_import_digests() -> TestResult {
    let first = prepare("legacy-reopened.sqlite").await?;
    let first_digests = first
        .report
        .projects
        .iter()
        .map(|project| project.digest.clone())
        .collect::<Vec<_>>();
    let first_fleet = first.report.fleet_digest.clone();
    let first_integrity = first
        .report
        .projects
        .iter()
        .map(|project| serde_json::to_value(&project.manifest))
        .collect::<Result<Vec<_>, _>>()?;
    first.close().await;
    let second = prepare("legacy-reopened.sqlite").await?;
    assert_eq!(
        first_digests,
        second
            .report
            .projects
            .iter()
            .map(|project| project.digest.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(first_fleet, second.report.fleet_digest);
    // A semantic digest alone cannot detect random storage helper keys. Compare
    // actual snapshot integrity too, with identical provenance and owner.
    assert_eq!(
        first_integrity,
        second
            .report
            .projects
            .iter()
            .map(|project| serde_json::to_value(&project.manifest))
            .collect::<Result<Vec<_>, _>>()?
    );
    second.close().await;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn source_bytes_and_modification_time_are_unchanged() -> TestResult {
    let (_directory, source) = fixture_copy("legacy-complete.sqlite")?;
    let before = std::fs::read(&source)?;
    let modified = source.metadata()?.modified()?;
    let prepared = prepare_path(&source).await?;
    prepared.close().await;
    assert_eq!(before, std::fs::read(&source)?);
    assert_eq!(modified, source.metadata()?.modified()?);
    Ok(())
}
