use message_board::*;
use serde_json::json;
mod support;

fn membership_value() -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let reader = Identity::Human {
        human_id: HumanId::try_from("reader".to_owned())?,
    };
    let project = ProjectId::generate();
    Ok(
        json!({"envelope":support::record_envelope(RecordId::generate(), project.clone()),
        "reader":reader,"scope":SubscriptionScope::project(project),
        "policy":SubscriptionPolicy::defaults_for(&reader),"generation":1,"activatedAt":{"epoch":1,"position":0},
        "renewedAt":"2026-10-12T00:00:00Z","expiresAt":"2026-10-13T00:00:00Z","ended":null}),
    )
}

#[expect(
    clippy::unwrap_in_result,
    reason = "This test double must fail on broken fixture invariants instead of reporting a product error."
)]
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)] // Assertions are the test contract; Result propagates fixture errors.
fn subscription_membership_contains_no_router_local_delivery_progress()
-> Result<(), Box<dyn std::error::Error>> {
    let value = membership_value()?;
    let record = json!({"kind":"subscriptionMembership","record":value});
    let decoded = serde_json::from_value::<Record>(record.clone());
    assert!(decoded.is_ok(), "project membership: {decoded:?}");
    assert_eq!(serde_json::to_value(decoded.unwrap()).unwrap(), record);
    let mut local = record;
    local["record"]["deliveredThrough"] = json!({"epoch":1,"position":42});
    assert!(serde_json::from_value::<Record>(local).is_err());
    Ok(())
}

#[expect(
    clippy::unwrap_in_result,
    reason = "This test double must fail on broken fixture invariants instead of reporting a product error."
)]
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)] // Assertions are the test contract; Result propagates fixture errors.
fn membership_expiry_and_draining_are_derived_without_an_automated_end_write()
-> Result<(), Box<dyn std::error::Error>> {
    let mut value = membership_value()?;
    let expiry: chrono::DateTime<chrono::Utc> = "2026-10-13T00:00:00Z".parse().unwrap();
    let before = expiry - chrono::Duration::seconds(1);
    let decoded = serde_json::from_value::<SubscriptionMembership>(value.clone());
    assert!(decoded.is_ok(), "membership: {decoded:?}");
    let membership = decoded.unwrap();
    assert_eq!(
        membership.state_at(before, None, false),
        SubscriptionState::Active
    );
    assert_eq!(
        membership.state_at(expiry, None, false),
        SubscriptionState::Ended {
            reason: EndReason::Expired
        }
    );
    assert_eq!(
        serde_json::to_value(&membership).unwrap(),
        value,
        "derivation writes no end"
    );
    value["ended"] = json!({"reason":{"kind":"resolved"},"position":{"epoch":1,"position":42},"at":"2026-10-12T00:10:00Z"});
    let ended: SubscriptionMembership = serde_json::from_value(value).unwrap();
    assert_eq!(
        ended.state_at(
            before,
            Some(FeedCursor {
                epoch: EpochNumber::try_from(1).unwrap(),
                position: FeedPosition::try_from(41).unwrap()
            }),
            true
        ),
        SubscriptionState::Draining
    );
    assert_eq!(
        ended.state_at(
            before,
            Some(FeedCursor {
                epoch: EpochNumber::try_from(1).unwrap(),
                position: FeedPosition::try_from(42).unwrap()
            }),
            true
        ),
        SubscriptionState::Ended {
            reason: EndReason::Resolved
        }
    );
    Ok(())
}
