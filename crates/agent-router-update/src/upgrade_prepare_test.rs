use super::*;
use message_board::{MachineLabel, Record, RecordKind, RecordedActor, ServiceId};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn empty_legacy_project_roundtrips_unknown_metadata_without_a_creation_fact()
-> Result<(), Box<dyn std::error::Error>> {
    let (_source_directory, source_path) = fixture_copy("role-history.sqlite")?;
    let source = read_legacy(source_path).await?;
    let project = source
        .projects
        .iter()
        .find(|project| project.topic_id.as_str().ends_with("0016"))
        .ok_or("empty project missing")?;
    let provenance = ImportProvenance {
        installation: ServiceId::try_from("550e8400-e29b-41d4-a716-446655440000".to_owned())?,
        machine: MachineLabel::try_from("offline import".to_owned())?,
    };
    let owner = HumanId::try_from("explicit-owner".to_owned())?;
    let directory = tempfile::tempdir()?;
    let (prepared, report) = prepare_project(
        &source,
        project,
        &provenance,
        &owner,
        &directory.path().join("project.sqlite"),
    )
    .await?;
    assert_eq!(report.feed_rows, 1);
    assert_eq!(report.posts, 0);
    let mut snapshot = prepared.store.snapshot().await?;
    let Some(Record::Project(state)) = snapshot.record(&project.topic_id.clone().into()).await?
    else {
        return Err("metadata missing".into());
    };
    assert_eq!(state.envelope.time, RecordTime::LegacyUnknown);
    assert_eq!(state.description, project.description);
    assert_eq!(
        state.envelope.actor,
        RecordedActor::Human { human_id: owner }
    );
    assert_eq!(
        state
            .envelope
            .position
            .project_cursor()
            .ok_or("placement missing")?
            .position
            .get(),
        1
    );
    assert!(
        snapshot
            .record_keys(RecordKind::Fact, "", 100)
            .await?
            .is_empty()
    );
    snapshot.finish().await?;
    prepared.database.close().await;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn collision_detection_keeps_both_ids_and_never_renames()
-> Result<(), Box<dyn std::error::Error>> {
    let (_first_directory, first) = fixture_copy("legacy-complete.sqlite")?;
    let (_second_directory, second) = fixture_copy("collision.sqlite")?;
    let sources = vec![read_legacy(first).await?, read_legacy(second).await?];
    let collisions = find_collisions(&sources);
    assert!(collisions.iter().any(|collision| matches!(
        collision.location,
        CollisionLocation::Workspace
    ) && collision.ids.len() == 2));
    assert!(collisions.iter().any(|collision| matches!(
        collision.location,
        CollisionLocation::Board { .. }
    ) && collision.ids.len() == 2));
    Ok(())
}

fn fixture_copy(name: &str) -> Result<(tempfile::TempDir, PathBuf), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let destination = directory.path().join(name);
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
        &destination,
    )?;
    Ok((directory, destination))
}
