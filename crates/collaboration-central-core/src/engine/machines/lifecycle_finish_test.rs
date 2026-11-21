use central_engine_model::{Context, Machine, MachineKind, TransitionError};
use central_engine_test_support::timer::{NoIds, context_now};
use fleet_schedules::AutomationFinish;
use fleet_timer_policy::{FinishEvent, Lifecycle, TimerRefusal};
use fleet_wakes::WakeFinish;

#[test]
fn discard_disables_an_active_subject_and_refuses_a_finished_one() {
    let mut context = Context {
        now: context_now(),
        ids: &mut NoIds,
    };
    assert_eq!(
        AutomationFinish::transition(&Lifecycle::Active, FinishEvent::Discard, &mut context),
        Ok((Lifecycle::Disabled, vec![]))
    );
    for finished in [Lifecycle::Done, Lifecycle::Disabled] {
        assert_eq!(
            AutomationFinish::transition(&finished, FinishEvent::Discard, &mut context),
            Err(TransitionError::Domain(TimerRefusal::AlreadyFinished))
        );
    }
    assert_eq!(<WakeFinish as Machine>::KIND, MachineKind("wake_timer"));
}
