#![expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions fail the test while fixture errors propagate."
)]
mod support;
use board_legacy_upgrade::prepare_sources;
use message_board::*;
use support::*;

#[tokio::test]
async fn relabelled_or_different_owner_rerun_keeps_semantic_digest_while_integrity_changes()
-> TestResult {
    let (_directory, path) = fixture_copy("role-history.sqlite")?;
    let first = prepare_sources(
        std::slice::from_ref(&path),
        provenance("first import label")?,
        HumanId::try_from("first-owner".to_owned())?,
    )
    .await?;
    let second = prepare_sources(
        &[path],
        provenance("second import label")?,
        HumanId::try_from("second-owner".to_owned())?,
    )
    .await?;
    let first_digests = first
        .report
        .projects
        .iter()
        .map(|project| &project.digest)
        .collect::<Vec<_>>();
    let second_digests = second
        .report
        .projects
        .iter()
        .map(|project| &project.digest)
        .collect::<Vec<_>>();
    assert_eq!(first_digests, second_digests);
    assert_eq!(first.report.fleet_digest, second.report.fleet_digest);
    // Sensitivity control: hashing the complete manifest includes stored origin
    // and attribution, and would falsely conflict on this allowed rerun.
    let first_integrity = first
        .report
        .projects
        .iter()
        .map(|project| fleet_sync_protocol::ImportDigest::of_canonical(&project.manifest))
        .collect::<Result<Vec<_>, _>>()?;
    let second_integrity = second
        .report
        .projects
        .iter()
        .map(|project| fleet_sync_protocol::ImportDigest::of_canonical(&project.manifest))
        .collect::<Result<Vec<_>, _>>()?;
    assert_ne!(first_integrity, second_integrity);
    first.close().await;
    second.close().await;
    Ok(())
}

#[tokio::test]
async fn changed_legacy_description_changes_the_semantic_import_identity() -> TestResult {
    let (_directory, path) = fixture_copy("role-history.sqlite")?;
    let first = prepare_path(&path).await?;
    let original = first
        .report
        .projects
        .iter()
        .map(|project| project.digest.clone())
        .collect::<Vec<_>>();
    first.close().await;
    let connection = rusqlite::Connection::open(&path)?;
    connection.execute("UPDATE board_topics SET description='Changed legacy description' WHERE topic_id='019a0000-0000-7000-8000-000000000015'",[])?;
    connection.close().map_err(|(_, error)| error)?;
    let changed = prepare_path(&path).await?;
    assert_ne!(
        original,
        changed
            .report
            .projects
            .iter()
            .map(|project| project.digest.clone())
            .collect::<Vec<_>>()
    );
    changed.close().await;
    Ok(())
}
