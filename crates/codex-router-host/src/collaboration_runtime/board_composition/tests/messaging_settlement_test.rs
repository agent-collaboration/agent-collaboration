//! Real Central authorizes Settle from the current holder using retained Project custody.
use super::*;
use board_central_test_support::MailWireEvent;
use collaboration_contracts::RouterDeliveryTypes;
use fleet_mail_delivery::{DispatchOutcome, OutboxAdmission};
use fleet_protocol::{DeliveryItemRef, DeliveryRefusal, SubmitResult, Submitter};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; real fixture errors propagate."
)]
async fn project_settle_uses_the_retained_local_project_owner_after_resolution_fails() -> TestResult
{
    let mut fixture = Box::pin(RuleFixture::open_with_deferred_successor()).await?;
    let proof = Box::pin(async {
        // Arrange actual old-holder custody, then a verified current-holder takeover.
        let seed = Box::pin(cancel_cases::arrange_sent_follow(&mut fixture)).await?;
        take_over_for_settlement(&mut fixture).await?;
        let original = &seed.item;
        let other = DeliveryItem::<BoardDeliveryTypes>::new(
            ItemId::new(durable_steps::Uuid::now_v7())?,
            original.sender().clone(),
            original.recipient().clone(),
            original.valid_until(),
            original.body().clone(),
        )?;
        let wrong_digest = other.digest().clone();
        assert_ne!(original.digest(), &wrong_digest);
        let local: fleet_protocol::DeliveryItem<RouterDeliveryTypes> =
            serde_json::from_value(serde_json::to_value(original)?)?;
        let runtime = fixture.successor.notifications.mail_runtime();
        let admitted = runtime
            .enqueue_project_item(local, 3.try_into()?)
            .await
            .map_err(|error| format!("retained project custody: {error:?}"))?;
        assert!(matches!(
            admitted,
            OutboxAdmission::Queued { .. } | OutboxAdmission::Replayed { .. }
        ));
        let observed = fixture.successor.observe_mail_wire()?;
        // Act through the production adapter; the mismatched reference cannot resolve locally.
        let outcome = runtime
            .runtime_test_submit_reference(
                original.recipient().peer,
                DeliveryItemRef {
                    item: original.id(),
                    digest: wrong_digest,
                },
            )
            .await;
        // Assert actual Access bytes and Central's exact canonical digest-conflict answer.
        let claims = observed
            .events()
            .into_iter()
            .filter_map(|event| match event {
                MailWireEvent::SettleSent {
                    item, submitter, ..
                } if item == original.id() => Some(submitter),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            claims,
            vec![Submitter::Project(DatabaseId::try_from(
                fixture.project.as_str().to_owned()
            )?)]
        );
        assert!(
            matches!(outcome?, DispatchOutcome::Answered(SubmitResult::Refused {
            reason: DeliveryRefusal::ItemConflict { existing }
        }) if existing == *original.digest())
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await;
    let closed = fixture.close().await;
    match (proof, closed) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(error)) => Err(error),
        (Err(error), Err(close)) => Err(format!("settlement: {error}; cleanup: {close}").into()),
    }
}

async fn take_over_for_settlement(fixture: &mut RuleFixture) -> TestResult {
    fixture.holder.disconnect_holder().await?;
    let moved = fixture
        .holder
        .central
        .take_over_project(
            DatabaseId::try_from(fixture.project.as_str().to_owned())?,
            &fixture.successor_origin.installation,
            TransferId::new(durable_steps::Uuid::now_v7())?,
        )
        .await?;
    if moved.pointer("/value/newEpoch") != Some(&json!(3)) {
        return Err("Settle fixture did not move to epoch3".into());
    }
    received_grants::wait_published_project(&fixture.successor.registry, &fixture.project, 3)
        .await?;
    Ok(())
}
