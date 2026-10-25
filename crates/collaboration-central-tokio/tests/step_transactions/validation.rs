//! Canonical encoding and a matching digest never make an invalid domain row valid.

use super::*;

fn valid_entry(suffix: &str) -> Result<RequestLedgerEntry, TestError> {
    let mut entry = collaboration_central_conformance::receipt_fixture(RequestId::try_from(
        format!("019f0000-0000-7000-8000-000000000{suffix}"),
    )?)?;
    // A Request record's identity is its request ID, independently of the
    // fixture's former random envelope ID.
    entry.envelope.id = RecordId::try_from(entry.request_id.as_str().to_owned())?;
    Ok(entry)
}
fn effects_request(entry: &RequestLedgerEntry) -> RequestEffectsRequest {
    RequestEffectsRequest {
        scope: RequestEffectsScope::Fleet {
            workspace_id: entry.envelope.scope.workspace.clone(),
        },
        request: entry.request_id.clone(),
        after: None,
    }
}
fn post_effect(entry: &RequestLedgerEntry) -> Result<DecisionEffect, TestError> {
    Ok((
        PostId::try_from("019f0000-0000-7000-8000-000000000fa0".to_owned())?.into(),
        entry.envelope.position.clone(),
    ))
}
async fn assert_first_write_refused(
    fixture: &Fixture,
    entry: RequestLedgerEntry,
    effects: Vec<DecisionEffect>,
) -> TestResult {
    let request = entry.request_id.clone();
    let result = fixture.driver.record_fleet_decision(entry, effects).await;
    assert!(
        matches!(result, Err(StoreError::Corrupt { .. })),
        "invalid first write was accepted: {result:?}"
    );
    assert_eq!(fixture.driver.read_fleet_decision(request).await?, None);
    assert_eq!(stored_receipt_count(fixture).await?, "0");
    Ok(())
}

#[tokio::test]
async fn an_empty_rejection_is_refused_before_its_first_write() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), true).await?;
    let mut entry = valid_entry("fa1")?;
    entry.decision = StoredDecision::Rejected {
        reasons: Vec::new(),
    };
    let result = assert_first_write_refused(&fixture, entry, Vec::new()).await;
    let closed = fixture.objects.close().await;
    result?;
    closed?;
    Ok(())
}

#[tokio::test]
async fn a_post_reference_is_refused_even_with_a_matching_header() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), true).await?;
    let mut entry = valid_entry("fa2")?;
    let effects = vec![post_effect(&entry)?];
    entry.decision = StoredDecision::accepted(effects.clone(), ChangeOutcome::Changed);
    let StoredDecision::Accepted {
        effects: header, ..
    } = &entry.decision
    else {
        return Err("test needs the matching accepted header".into());
    };
    assert!(header.matches_full(&effects));
    let result = assert_first_write_refused(&fixture, entry, effects).await;
    let closed = fixture.objects.close().await;
    result?;
    closed?;
    Ok(())
}

#[tokio::test]
async fn a_mismatched_request_envelope_id_is_refused_before_write() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), true).await?;
    let mut entry = valid_entry("fa3")?;
    entry.envelope.id = RecordId::try_from("019f0000-0000-7000-8000-000000000fa4".to_owned())?;
    let result = assert_first_write_refused(&fixture, entry, Vec::new()).await;
    let closed = fixture.objects.close().await;
    result?;
    closed?;
    Ok(())
}

async fn overwrite_payload(fixture: &Fixture, entry: &RequestLedgerEntry) -> TestResult {
    let request = entry.request_id.as_str().to_owned();
    let payload = serde_json::to_string(entry)?;
    overwrite_encoded_payload(fixture, request, payload).await
}

async fn overwrite_encoded_payload(
    fixture: &Fixture,
    request: String,
    payload: String,
) -> TestResult {
    fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                query::<sqlx_turso::Turso>(
                    "UPDATE fleet_request_decisions SET payload=? WHERE request_id=?",
                )
                .bind(payload)
                .bind(request)
                .execute(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                Ok(())
            })
        })
        .await?;
    Ok(())
}

#[tokio::test]
async fn stored_origin_text_is_refused_instead_of_normalized_on_read_page_and_replay() -> TestResult
{
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), true).await?;
    let entry = valid_entry("fa7")?;
    fixture
        .driver
        .record_fleet_decision(entry.clone(), Vec::new())
        .await?;
    let encoded = serde_json::to_string(&entry)?;
    assert_eq!(encoded.matches("\"machine\":\"fixture\"").count(), 1);
    let corrupted = encoded.replace("\"machine\":\"fixture\"", "\"machine\":\" fixture \"");
    overwrite_encoded_payload(&fixture, entry.request_id.as_str().to_owned(), corrupted).await?;
    let result = assert_stored_value_refused(&fixture, entry, Vec::new()).await;
    let closed = fixture.objects.close().await;
    result?;
    closed?;
    Ok(())
}

async fn assert_stored_value_refused(
    fixture: &Fixture,
    original: RequestLedgerEntry,
    effects: Vec<DecisionEffect>,
) -> TestResult {
    // Run all three public conformance observations before asserting, so a
    // replay rejection cannot hide an independently accepting page decoder.
    let read = fixture
        .driver
        .read_fleet_decision(original.request_id.clone())
        .await;
    let page = fixture
        .driver
        .read_fleet_effects(effects_request(&original))
        .await;
    let replay = fixture
        .driver
        .record_fleet_decision(original, effects)
        .await;
    assert!(
        matches!(read, Err(StoreError::Corrupt { .. })),
        "full read accepted invalid storage: {read:?}"
    );
    assert!(
        matches!(page, Err(StoreError::Corrupt { .. })),
        "page accepted invalid storage: {page:?}"
    );
    assert!(
        matches!(replay, Err(StoreError::Corrupt { .. })),
        "full replay accepted invalid storage: {replay:?}"
    );
    Ok(())
}

#[tokio::test]
async fn a_stored_empty_rejection_is_refused_on_read_page_and_replay() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), true).await?;
    let mut entry = valid_entry("fa5")?;
    entry.decision = StoredDecision::Rejected {
        reasons: vec![Refusal::Archived],
    };
    fixture
        .driver
        .record_fleet_decision(entry.clone(), Vec::new())
        .await?;
    let mut corrupt = entry.clone();
    corrupt.decision = StoredDecision::Rejected {
        reasons: Vec::new(),
    };
    overwrite_payload(&fixture, &corrupt).await?;
    let result = assert_stored_value_refused(&fixture, entry, Vec::new()).await;
    let closed = fixture.objects.close().await;
    result?;
    closed?;
    Ok(())
}

#[tokio::test]
async fn a_stored_post_effect_with_a_matching_digest_is_refused_on_page_and_replay() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), true).await?;
    let mut entry = valid_entry("fa6")?;
    let effects = vec![(
        entry.envelope.scope.workspace.clone().into(),
        entry.envelope.position.clone(),
    )];
    entry.decision = StoredDecision::accepted(effects.clone(), ChangeOutcome::Changed);
    fixture
        .driver
        .record_fleet_decision(entry.clone(), effects.clone())
        .await?;
    let bad_effects = vec![post_effect(&entry)?];
    let mut corrupt = entry.clone();
    corrupt.decision = StoredDecision::accepted(bad_effects.clone(), ChangeOutcome::Changed);
    let StoredDecision::Accepted {
        effects: header, ..
    } = &corrupt.decision
    else {
        return Err("test needs the recomputed header".into());
    };
    assert!(header.matches_full(&bad_effects));
    overwrite_payload(&fixture, &corrupt).await?;
    let request = entry.request_id.as_str().to_owned();
    let effect = serde_json::to_string(bad_effects.first().ok_or("one corrupt effect required")?)?;
    fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                query::<sqlx_turso::Turso>(
                    "UPDATE fleet_decision_receipts SET effect=? WHERE request_id=? AND ordinal=0",
                )
                .bind(effect)
                .bind(request)
                .execute(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                Ok(())
            })
        })
        .await?;
    let result = assert_stored_value_refused(&fixture, entry, effects).await;
    let closed = fixture.objects.close().await;
    result?;
    closed?;
    Ok(())
}
