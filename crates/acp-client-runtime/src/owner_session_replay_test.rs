use super::*;

#[test]
fn replay_output_limit_remains_a_post_dispatch_failure() {
    let failure = replay_projection_error(ItemProjectionError::OutputLimit);
    assert!(matches!(
        failure,
        ExternalProviderRuntimeError::HistoryReplayUnavailable
    ));
}
