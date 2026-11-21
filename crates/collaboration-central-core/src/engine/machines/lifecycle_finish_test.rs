use central_engine_model::{Context, Machine, MachineKind, TransitionError};
use central_engine_test_support::timer::{NoIds, context_now};
use fleet_schedules::AutomationKind;
use fleet_timer_policy::{FinishEvent, Lifecycle, LifecycleFinish, TimerRefusal};
use fleet_wakes::WakeKind;

#[test]
fn discard_disables_an_active_subject_and_refuses_a_finished_one() {
    let mut context = Context {
        now: context_now(),
        ids: &mut NoIds,
    };
    assert_eq!(
        LifecycleFinish::<AutomationKind>::transition(
            &Lifecycle::Active,
            FinishEvent::Discard,
            &mut context
        ),
        Ok((Lifecycle::Disabled, vec![]))
    );
    for finished in [Lifecycle::Done, Lifecycle::Disabled] {
        assert_eq!(
            LifecycleFinish::<AutomationKind>::transition(
                &finished,
                FinishEvent::Discard,
                &mut context
            ),
            Err(TransitionError::Domain(TimerRefusal::AlreadyFinished))
        );
    }
    assert_eq!(
        <LifecycleFinish<WakeKind> as Machine>::KIND,
        MachineKind("wake_timer")
    );
}
