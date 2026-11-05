use super::{ExternalProviderRuntimeError, ItemProjectionError, replay_projection_error};

#[test]
fn replay_output_limit_remains_a_post_dispatch_failure() {
    // Arrange/Act: this conversion happens only after the ACP load returned replayed updates.
    let error = replay_projection_error(ItemProjectionError::OutputLimit);
    // Assert: this must never become the new pre-load admission failure.
    assert!(matches!(
        error,
        ExternalProviderRuntimeError::HistoryReplayUnavailable
    ));
}
