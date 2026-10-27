#![expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions fail the test while fixture errors propagate."
)]
mod support;
use message_board::*;
use support::*;

#[tokio::test]
async fn shipped_history_with_an_unrecoverable_earlier_join_role_is_importable() -> TestResult {
    // Arrange: synthetic actor B joined as advisor, then reviewer. The shipped
    // 202610020001 migration's Step 3 can recover only the surviving join at 4;
    // the earlier join at 3 legitimately keeps a NULL role after that migration.
    let (_directory, path) = fixture_copy("role-history.sqlite")?;
    let source = rusqlite::Connection::open(&path)?;
    assert_eq!(
        source.execute(
            "UPDATE board_activity SET participant_role=NULL WHERE activity_sequence=3",
            [],
        )?,
        1
    );
    source.close().map_err(|(_, error)| error)?;
    let before = std::fs::read(&path)?;

    // Act: call the real importer on the legal shipped shape, without inferring
    // the lost role from the surviving Participant's later role.
    let prepared = prepare_path(&path).await?;

    // Assert: refusing all migration output is wrong; no source repair occurs.
    assert_eq!(prepared.projects.len(), 2);
    let project = prepared
        .projects
        .iter()
        .find(|project| project.store.project_id().as_str().ends_with("0015"))
        .ok_or("project missing")?;
    let thread = ThreadId::try_from("019a0000-0000-7000-8000-000000000065".to_owned())?;
    let mut snapshot = project.store.snapshot().await?;
    let intervals = snapshot.participants(&thread).await?;
    let mut actor_b = intervals.iter().filter(|row| {
        matches!(&row.identity, Identity::Session { session } if session.session_id.as_str() == "actor-b")
    }).collect::<Vec<_>>();
    actor_b.sort_by_key(|row| row.joined_at_position.position);
    assert_eq!(
        actor_b.len(),
        3,
        "a lost role must not discard its interval"
    );
    let [earlier, next, _] = actor_b.as_slice() else {
        return Err("intervals missing".into());
    };
    assert_eq!(earlier.role, RecordedParticipantRole::NotRecorded);
    assert_eq!(
        earlier.closed_reason,
        Some(ParticipantClosedReason::Rejoined)
    );
    assert_eq!(earlier.closed_at_position, Some(next.joined_at_position));
    assert_eq!(
        next.role,
        RecordedParticipantRole::Recorded(ParticipantRole::Reviewer)
    );
    assert_eq!(
        serde_json::to_value(earlier)?["role"],
        serde_json::json!({"kind":"notRecorded"})
    );
    snapshot.finish().await?;
    assert_eq!(std::fs::read(&path)?, before);
    prepared.close().await;
    Ok(())
}

#[tokio::test]
async fn open_interval_uses_only_its_current_participant_rows_role() -> TestResult {
    let (_directory, path) = fixture_copy("role-history.sqlite")?;
    let source = rusqlite::Connection::open(&path)?;
    source.execute(
        "UPDATE board_activity SET participant_role=NULL WHERE activity_sequence=7",
        [],
    )?;
    source.close().map_err(|(_, error)| error)?;
    let prepared = prepare_path(&path).await?;
    let project = prepared
        .projects
        .iter()
        .find(|project| project.store.project_id().as_str().ends_with("0015"))
        .ok_or("project missing")?;
    let thread = ThreadId::try_from("019a0000-0000-7000-8000-000000000066".to_owned())?;
    let mut snapshot = project.store.snapshot().await?;
    let intervals = snapshot.participants(&thread).await?;
    assert_eq!(intervals.len(), 1);
    let interval = intervals.first().ok_or("open interval missing")?;
    assert!(interval.is_open());
    assert_eq!(
        interval.role,
        RecordedParticipantRole::Recorded(ParticipantRole::Orchestrator)
    );
    snapshot.finish().await?;
    prepared.close().await;
    Ok(())
}

#[tokio::test]
async fn open_interval_without_a_current_role_is_refused_without_source_repair() -> TestResult {
    let (_directory, path) = fixture_copy("role-history.sqlite")?;
    let source = rusqlite::Connection::open(&path)?;
    source.execute_batch("UPDATE board_activity SET participant_role=NULL WHERE activity_sequence=7; DELETE FROM thread_participants WHERE joined_at_activity=7;")?;
    source.close().map_err(|(_, error)| error)?;
    let before = std::fs::read(&path)?;
    let result = prepare_path(&path).await;
    match result {
        Ok(prepared) => {
            prepared.close().await;
            return Err("unknown open role accepted".into());
        }
        Err(error) => assert!(
            matches!(error.downcast_ref::<board_legacy_upgrade::UpgradeError>(), Some(board_legacy_upgrade::UpgradeError::InvalidLegacy { field, .. }) if field == "participant_role")
        ),
    }
    assert_eq!(std::fs::read(path)?, before);
    Ok(())
}
