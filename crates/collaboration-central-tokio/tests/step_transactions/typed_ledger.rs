//! Shared literal refusal trees run through the real native transaction owner.
use super::*;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Typed headers, reason graph and ordered complete/abridged evidence must survive rollback, replay and owning reopen."
)]
async fn typed_ledger_trees_rollback_replay_and_reopen_through_native_owner() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), true).await?;
    let entries = collaboration_central_conformance::typed_ledger_fixtures()?;
    for entry in &entries {
        assert!(matches!(
            fixture
                .driver
                .record_with_fault(entry.clone(), Vec::new(), FleetDecisionFault::SqlAfterWrite)
                .await,
            Err(StoreError::Backend { .. })
        ));
        assert_eq!(
            fixture
                .driver
                .read_fleet_decision(entry.request_id.clone())
                .await?,
            None
        );
        fixture
            .driver
            .record_fleet_decision(entry.clone(), Vec::new())
            .await?;
        assert_eq!(
            fixture
                .driver
                .read_fleet_decision(entry.request_id.clone())
                .await?,
            Some(entry.clone())
        );
    }
    let before = fixture
        .object
        .transaction(|scope| {
            Box::pin(async move { crate::fleet_ledger_rows::in_scope(scope).await })
        })
        .await?;
    fixture.objects.close().await?;
    drop(fixture);
    let fixture = Fixture::open(root.path(), false).await?;
    for entry in entries {
        assert_eq!(
            fixture
                .driver
                .read_fleet_decision(entry.request_id.clone())
                .await?,
            Some(entry.clone())
        );
        let answer = fixture
            .driver
            .record_fleet_decision(entry.clone(), Vec::new())
            .await?;
        let mut changed = entry;
        changed.decision = StoredDecision::accepted(Vec::new(), ChangeOutcome::Changed);
        assert_eq!(
            fixture
                .driver
                .record_fleet_decision(changed.clone(), Vec::new())
                .await?,
            answer
        );
        changed.canonical_hash = "ef".repeat(32).try_into()?;
        assert_eq!(
            fixture
                .driver
                .record_fleet_decision(changed, Vec::new())
                .await?,
            vec![FleetRequestResult::ConflictingRequest]
        );
    }
    let after = fixture
        .object
        .transaction(|scope| {
            Box::pin(async move { crate::fleet_ledger_rows::in_scope(scope).await })
        })
        .await?;
    assert_eq!(after, before);
    fixture.objects.close().await?;
    Ok(())
}
