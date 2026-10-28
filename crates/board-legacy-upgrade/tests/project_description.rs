mod support;
use board_legacy_upgrade::UpgradeError;
use message_board::*;
use support::*;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn descriptions_roundtrip_exactly_in_native_project_state_and_title_only_directory()
-> TestResult {
    let maximum = "λ\0\n".repeat(4096);
    assert_eq!(maximum.len(), MAX_DESCRIPTION_BYTES);
    for (topic, description) in [
        ("019a0000-0000-7000-8000-000000000016", String::new()),
        (
            "019a0000-0000-7000-8000-000000000015",
            "  first\r\nsecond\0\tline\n ".to_owned(),
        ),
        ("019a0000-0000-7000-8000-000000000016", maximum),
    ] {
        let (_directory, path) = fixture_copy("role-history.sqlite")?;
        let source = rusqlite::Connection::open(&path)?;
        source.execute(
            "UPDATE board_topics SET description=? WHERE topic_id=?",
            [&description, topic],
        )?;
        source.close().map_err(|(_, error)| error)?;
        let bytes = std::fs::read(&path)?;
        let prepared = prepare_path(&path).await?;
        let project = prepared
            .projects
            .iter()
            .find(|project| project.store.project_id().as_str() == topic)
            .ok_or("project missing")?;
        let mut snapshot = project.store.snapshot().await?;
        let Some(Record::Project(state)) = snapshot
            .record(&project.store.project_id().clone().into())
            .await?
        else {
            return Err("ProjectState missing".into());
        };
        assert_eq!(state.description.as_str(), description);
        assert_eq!(state.envelope.time, RecordTime::LegacyUnknown);
        assert_eq!(
            state
                .envelope
                .position
                .project_cursor()
                .ok_or("cursor missing")?
                .position
                .get(),
            1
        );
        snapshot.finish().await?;
        for record in &prepared.report.fleet.records {
            if matches!(record, fleet_admission::FleetImportRecord::Directory { .. }) {
                let wire = serde_json::to_value(record)?;
                assert!(wire.get("description").is_none());
                assert!(wire.get("title").is_some());
            }
        }
        prepared.close().await;
        assert_eq!(bytes, std::fs::read(path)?);
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn invalid_legacy_description_is_a_topic_named_typed_refusal_without_truncation() -> TestResult
{
    let (_directory, path) = fixture_copy("role-history.sqlite")?;
    let topic = "019a0000-0000-7000-8000-000000000016";
    let source = rusqlite::Connection::open(&path)?;
    source.execute(
        "UPDATE board_topics SET description=? WHERE topic_id=?",
        [&"x".repeat(MAX_DESCRIPTION_BYTES + 1), topic],
    )?;
    source.close().map_err(|(_, error)| error)?;
    let bytes = std::fs::read(&path)?;
    let modified = path.metadata()?.modified()?;
    match prepare_path(&path).await {
        Ok(prepared) => {
            prepared.close().await;
            return Err("oversized legacy description was accepted".into());
        }
        Err(error) => assert!(
            matches!(error.downcast_ref::<UpgradeError>(), Some(UpgradeError::InvalidLegacyTopic { topic: id, .. }) if id.as_str() == topic),
            "expected topic-named typed refusal, got {error}"
        ),
    }
    assert_eq!(bytes, std::fs::read(&path)?);
    assert_eq!(modified, path.metadata()?.modified()?);
    Ok(())
}
