use super::*;
use central_engine_model::{IdSource, check_transitions};

struct NoIds;
impl IdSource for NoIds {
    fn next_id(&mut self) -> uuid::Uuid {
        uuid::Uuid::from_u128(0x01890f2e7b4c7cc098c4000000000001)
    }
}

fn context_now() -> EngineTime {
    EngineTime::from_timestamp(1_794_571_200, 0).unwrap()
}

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
