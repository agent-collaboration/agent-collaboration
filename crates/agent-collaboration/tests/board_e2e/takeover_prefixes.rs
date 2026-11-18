use codex_router_test_support::FixtureResult;
use fleet_sync_protocol::ReplicationPosition;
use serde_json::json;

fn discarded_copy_excludes_cause(
    cause: Option<ReplicationPosition>,
    copy: ReplicationPosition,
) -> bool {
    cause.is_some_and(|cause| copy.epoch == cause.epoch && copy.sequence < cause.sequence)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[test]
fn discarded_copy_rejects_a_missing_cause_capture() -> FixtureResult {
    let ack_before_crash = serde_json::from_value(json!({"epoch":1,"sequence":99}))?;
    let advanced_ack = serde_json::from_value(json!({"epoch":1,"sequence":100}))?;
    let regressed_ack = serde_json::from_value(json!({"epoch":1,"sequence":98}))?;
    let ack_in_other_epoch = serde_json::from_value(json!({"epoch":2,"sequence":99}))?;
    assert!(!discarded_copy_excludes_cause(None, ack_before_crash));
    assert!(!discarded_copy_excludes_cause(None, advanced_ack));
    assert!(!discarded_copy_excludes_cause(None, regressed_ack));
    assert!(!discarded_copy_excludes_cause(None, ack_in_other_epoch));
    Ok(())
}
