use super::*;
use central_engine_model::{Context, check_transitions};
use central_engine_test_support::timer::{NoIds, context_now};
use fleet_timer_policy::FinishEvent;

#[test]
fn every_lifecycle_state_meets_discard_exactly_as_declared() {
    let cases = [Lifecycle::Active, Lifecycle::Done, Lifecycle::Disabled]
        .into_iter()
        .map(|state| (state, FinishEvent::Discard));
    assert_eq!(
        check_transitions::<LifecycleFinish<WakeKind>>(
            cases,
            &mut Context {
                now: context_now(),
                ids: &mut NoIds
            }
        ),
        Ok(())
    );
}
