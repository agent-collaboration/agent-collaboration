#![allow(clippy::panic_in_result_fn)]
mod support;
use board_legacy_upgrade::UpgradeError;
use support::*;

#[tokio::test]
async fn corrupt_stored_attribution_never_becomes_an_inferred_role() -> TestResult {
    // These are the shipped decoder's seven fail-closed relationships, arranged
    // in real ffced1c1 files, as its participant_history_read_tests did.
    for (case, mutation) in [
        (
            "invalid role",
            "UPDATE board_activity SET participant_role='bogus' WHERE activity_sequence=5",
        ),
        (
            "wrong author",
            "UPDATE board_messages SET posted_from_activity=2 WHERE message_id='019a0000-0000-7000-8000-0000000000c9'",
        ),
        (
            "wrong root",
            "UPDATE board_activity SET root_id='019a0000-0000-7000-8000-000000000066' WHERE activity_sequence=2",
        ),
        (
            "later sequence",
            "INSERT INTO board_activity(activity_sequence,project_id,board_id,topic_id,root_id,kind,actor_key,message_id,participant_key,participant_role,replaced_participant_key) SELECT 100,project_id,board_id,topic_id,root_id,'participantJoined',participant_key,NULL,participant_key,'advisor',NULL FROM board_activity WHERE activity_sequence=5; UPDATE activity_checkpoint SET last_sequence=100; UPDATE board_messages SET posted_from_activity=100 WHERE message_id='019a0000-0000-7000-8000-0000000000c9'",
        ),
        (
            "missing role",
            "UPDATE board_activity SET participant_role=NULL WHERE activity_sequence=5",
        ),
        (
            "main wrong kind",
            "UPDATE board_activity SET kind='orchestratorReplaced' WHERE activity_sequence=2",
        ),
        (
            "human with pointer",
            "UPDATE board_messages SET posted_from_activity=7 WHERE message_id='019a0000-0000-7000-8000-000000000066'",
        ),
    ] {
        let (_directory, path) = fixture_copy("watch-scope.sqlite")?;
        let source = rusqlite::Connection::open(&path)?;
        source.execute_batch(mutation)?;
        source.close().map_err(|(_, error)| error)?;
        let bytes = std::fs::read(&path)?;
        match prepare_path(&path).await {
            Ok(prepared) => {
                prepared.close().await;
                return Err(format!("corrupt {case} attribution was accepted").into());
            }
            Err(error) => assert!(
                matches!(error.downcast_ref::<UpgradeError>(), Some(UpgradeError::InvalidLegacy { field, .. }) if field != "schema"),
                "{case}: expected data refusal, got {error}",
            ),
        }
        assert_eq!(bytes, std::fs::read(path)?, "{case}");
    }
    Ok(())
}
