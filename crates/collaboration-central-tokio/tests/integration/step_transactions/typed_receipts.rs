//! Original four-kind effects survive the owning Native store and bounded page API.
use super::*;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "All original receipt kinds, order, exact positions above JS integer precision and full commitment must survive real Native reopen and page/replay reads."
)]
async fn four_typed_receipt_kinds_keep_original_placements_through_native_reopen_and_pages()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), true).await?;
    let (entry, effects) = collaboration_central_conformance::typed_receipt_fixture(
        "019f0000-0000-7000-8000-00000000cf01"
            .to_owned()
            .try_into()?,
    )?;
    let answer = fixture
        .driver
        .record_fleet_decision(entry.clone(), effects.clone())
        .await?;
    fixture.objects.close().await?;
    drop(fixture);
    let fixture = Fixture::open(root.path(), false).await?;
    let page = fixture
        .driver
        .read_fleet_effects(RequestEffectsRequest {
            scope: RequestEffectsScope::Fleet {
                workspace_id: entry.envelope.scope.workspace.clone(),
            },
            request: entry.request_id.clone(),
            after: None,
        })
        .await?;
    let RequestEffectsRead::Decision {
        decision:
            RequestDecision::Accepted {
                effects: page,
                change,
            },
    } = page
    else {
        return Err("typed receipt page missing".into());
    };
    assert_eq!(change, ChangeOutcome::Changed);
    assert_eq!(page.entries(), effects.as_slice());
    assert_eq!(page.total_count(), 4);
    assert_eq!(page.next(), None);
    let mut changed = entry.clone();
    changed.decision = StoredDecision::Rejected {
        reasons: vec![Refusal::Archived],
    };
    assert_eq!(
        fixture
            .driver
            .record_fleet_decision(changed, Vec::new())
            .await?,
        answer
    );
    assert_eq!(
        fixture
            .driver
            .read_fleet_decision(entry.request_id.clone())
            .await?,
        Some(entry)
    );
    fixture.objects.close().await?;
    Ok(())
}
