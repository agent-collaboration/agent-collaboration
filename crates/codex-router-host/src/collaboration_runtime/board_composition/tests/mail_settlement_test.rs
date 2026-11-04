//! Negative resolution keeps the validated owner from local custody on the actual wire.
use super::*;
use fleet_sync_delivery::DispatchOutcome;
use fleet_sync_protocol::{DeliveryFailureKind, DeliveryItemRef, ItemId};
use transport::WireEvent;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; setup propagates errors."
)]
async fn missing_local_submitter_is_unresolvable_without_sending_settle() -> Result<(), TestError> {
    // Arrange a real welcomed session with no local custody for this identity.
    let mut harness = MailHarness::open(false).await?;
    pushed_mail::welcome(&mut harness).await?;
    let item = ItemId::new(uuid::Uuid::now_v7())?;
    let digest = relay_items::ordinary_message(harness.peer())
        .digest()
        .clone();
    // Act through the same production adapter as a journaled Submit call.
    let outcome = harness
        .mail
        .mail
        .runtime_test_submit_reference(harness.peer(), DeliveryItemRef { item, digest })
        .await?;
    // Assert no ownership is invented and no remote negative record can be written.
    assert!(matches!(
        outcome,
        DispatchOutcome::Unresolvable {
            reason: DeliveryFailureKind::CustodyUnresolvable
        }
    ));
    assert!(
        !harness
            .transport
            .observer
            .events()
            .iter()
            .any(|event| matches!(event, WireEvent::Settle { .. }))
    );
    harness.close().await
}
