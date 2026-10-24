//! The mail codec preserves the producer's existing twenty-root batch bound.
#![allow(clippy::panic_in_result_fn)] // Assertions are test oracles; Result propagates fixture errors.
use collaboration_mail_protocol::{FollowDeliveryPolicy, NotificationSource, RootRange};
use message_board::{
    BatchTiming, FeedCursor, Identity, MAX_SUBSCRIPTION_NOTICE_ROOTS, PendingRootNotice, ProjectId,
    SubscriptionBatch, SubscriptionGeneration, SubscriptionLifetime, SubscriptionMode, ThreadId,
    WhenIdle,
};
use serde_json::{Value, json};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn cursor(position: u64) -> TestResult<FeedCursor> {
    Ok(serde_json::from_value(
        json!({"epoch":1,"position":position}),
    )?)
}

fn follow_batch_inputs(root_count: usize) -> TestResult<(SubscriptionBatch, FollowDeliveryPolicy)> {
    let project = ProjectId::generate();
    let roots = (0..root_count)
        .map(|_| {
            PendingRootNotice::new(
                ThreadId::generate(),
                project.clone(),
                cursor(2)?,
                cursor(3)?,
                2,
            )
            .map_err(Into::into)
        })
        .collect::<TestResult<Vec<_>>>()?;
    let batch = serde_json::from_value(json!({
        "batchId":"01900000-0000-7000-8000-000000000020",
        "held":false,"heldSince":null,"draining":false,"roots":roots
    }))?;
    let reader: Identity = serde_json::from_value(json!({
        "kind":"session","session":{
            "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},
            "sessionId":"follow-reader"
        }
    }))?;
    let policy = FollowDeliveryPolicy::new(
        &reader,
        SubscriptionMode::Deliver,
        WhenIdle::Hold,
        BatchTiming::new(2, 10)?,
        SubscriptionLifetime::new(600)?,
    )?;
    Ok((batch, policy))
}

fn follow_source_wire(
    batch: &SubscriptionBatch,
    policy: &FollowDeliveryPolicy,
) -> TestResult<Value> {
    let roots = batch
        .roots
        .iter()
        .map(RootRange::try_from)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(json!({
        "kind":"followBatch","membership":"01900000-0000-7000-8000-000000000021",
        "activatedAt":cursor(1)?,"generation":1,"roots":roots,"policy":policy,"draining":false
    }))
}

#[test]
fn follow_batch_construction_refuses_more_roots_than_the_producer_bound() -> TestResult {
    let (allowed, policy) = follow_batch_inputs(MAX_SUBSCRIPTION_NOTICE_ROOTS)?;
    let membership = "01900000-0000-7000-8000-000000000021"
        .to_owned()
        .try_into()?;
    let generation = SubscriptionGeneration::new(1)?;
    let source =
        NotificationSource::follow_batch(membership, cursor(1)?, generation, policy, &allowed)?;
    assert!(
        matches!(source, NotificationSource::FollowBatch { roots, .. } if roots.len() == MAX_SUBSCRIPTION_NOTICE_ROOTS)
    );

    let (oversized, policy) = follow_batch_inputs(MAX_SUBSCRIPTION_NOTICE_ROOTS + 1)?;
    let error = NotificationSource::follow_batch(
        "01900000-0000-7000-8000-000000000021"
            .to_owned()
            .try_into()?,
        cursor(1)?,
        generation,
        policy,
        &oversized,
    )
    .err()
    .ok_or("construction accepted a twenty-one-root follow batch")?;
    assert_eq!(error.field, "roots");
    Ok(())
}

#[test]
fn follow_batch_wire_refuses_more_roots_without_truncating_the_allowed_batch() -> TestResult {
    let (allowed, policy) = follow_batch_inputs(MAX_SUBSCRIPTION_NOTICE_ROOTS)?;
    let wire = follow_source_wire(&allowed, &policy)?;
    let decoded: NotificationSource = serde_json::from_value(wire.clone())?;
    assert_eq!(serde_json::to_value(decoded)?, wire);

    let (oversized, policy) = follow_batch_inputs(MAX_SUBSCRIPTION_NOTICE_ROOTS + 1)?;
    let error =
        serde_json::from_value::<NotificationSource>(follow_source_wire(&oversized, &policy)?)
            .err()
            .ok_or("wire decoding accepted a twenty-one-root follow batch")?;
    assert!(
        error.to_string().contains("roots"),
        "wrong refusal: {error}"
    );
    Ok(())
}

#[test]
fn root_range_preserves_mixed_assigning_epochs_and_refuses_decreasing_epochs() -> TestResult {
    let from: FeedCursor = serde_json::from_value(json!({"epoch":1,"position":3}))?;
    let through: FeedCursor = serde_json::from_value(json!({"epoch":2,"position":6}))?;
    let range = RootRange::new(ThreadId::generate(), from, through, 2)?;
    let wire = serde_json::to_vec(&range)?;
    let restored: RootRange = serde_json::from_slice(&wire)?;
    assert_eq!(restored, range);
    let reversed_epoch: FeedCursor = serde_json::from_value(json!({"epoch":3,"position":3}))?;
    assert!(RootRange::new(ThreadId::generate(), reversed_epoch, through, 2).is_err());
    Ok(())
}
