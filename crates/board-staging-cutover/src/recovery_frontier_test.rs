//! A singleton cannot claim settlement beyond the actual handoff head.
use super::*;
use fleet_sync_protocol::SqlValue;
use std::collections::BTreeMap;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The assertion checks a pure refusal before any custody owner is opened; errors propagate fixture setup failures."
)]
async fn recovery_frontier_refuses_a_prefix_above_the_handoff_head()
-> Result<(), Box<dyn std::error::Error>> {
    // Arrange a claimed frontier at one with no corresponding handoff.
    let delta = RecoveryDelta {
        added: BTreeMap::from([(
            "handoff_frontier".to_owned(),
            vec![vec![
                SqlValue::Text(uuid::Uuid::now_v7().to_string()),
                SqlValue::Integer(1),
            ]],
        )]),
        intents: vec![],
        source_emitted: 0,
        existing_actor: None,
        source_handoff_head: 0,
        source_frontier: None,
        changed_frontier: None,
    };
    let directory = tempfile::tempdir()?;
    let service = uuid::Uuid::now_v7().to_string().try_into()?;
    let peer = PeerId::try_from(uuid::Uuid::now_v7().to_string())?;
    // Act before any custody file can be opened or created.
    let result = verify_frontier(
        &delta,
        &directory.path().join("absent.sqlite"),
        &service,
        peer,
    )
    .await;
    // Assert the invalid project prefix itself is refused.
    assert!(matches!(
        result,
        Err(RecoveryError::Invalid("frontier range"))
    ));
    assert!(!directory.path().join("absent.sqlite").exists());
    Ok(())
}
