use super::*;
use central_engine_model::{IdSource, check_transitions};
struct ReplayIds;
impl IdSource for ReplayIds {
    fn next_id(&mut self) -> uuid::Uuid {
        uuid::Uuid::from_u128(0x01890f2e7b4c7cc098c4000000000001)
    }
}
fn time(second: i64) -> EngineTime {
    EngineTime::from_timestamp(second, 0).unwrap()
}
fn waiting(due: Option<EngineTime>) -> ProbeState {
    ProbeState::Waiting {
        group: "01890f2e-7b4c-7cc0-98c4-000000000001".parse().unwrap(),
        due,
        claim: "one".into(),
    }
}
#[test]
fn every_probe_state_event_pair_matches_the_declared_edges() {
    let cases = [
        waiting(Some(time(300))),
        waiting(None),
        ProbeState::Fired,
        ProbeState::Stopped,
    ]
    .into_iter()
    .flat_map(|state| {
        [
            ProbeEvent::Due {
                occurrence: time(300),
            },
            ProbeEvent::Run,
            ProbeEvent::Stop,
        ]
        .into_iter()
        .map(move |event| (state.clone(), event))
    });
    assert_eq!(
        check_transitions::<EngineProbe>(
            cases,
            &mut Context {
                now: time(300),
                ids: &mut ReplayIds
            }
        ),
        Ok(())
    );
}
#[hegel::test]
fn probe_sequences_are_declared_deterministic_and_finish_once(case: hegel::TestCase) {
    use hegel::generators as gs;
    let mut state = match case.draw(gs::integers::<u8>().max_value(3)) {
        0 => waiting(Some(time(300))),
        1 => waiting(None),
        2 => ProbeState::Fired,
        _ => ProbeState::Stopped,
    };
    let mut effects = 0;
    for step in 0..case.draw(gs::integers::<u8>().min_value(1).max_value(50)) {
        let now = time(case.draw(gs::integers::<i64>().min_value(0).max_value(604_800)));
        let event = match case.draw(gs::integers::<u8>().max_value(3)) {
            0 => ProbeEvent::Due {
                occurrence: time(300),
            },
            1 => ProbeEvent::Due { occurrence: now },
            2 => ProbeEvent::Run,
            _ => ProbeEvent::Stop,
        };
        match central_engine_test_support::checked_step::<EngineProbe>(
            &state,
            event,
            now,
            u64::from(step),
        ) {
            Ok((next, produced)) => {
                effects += produced
                    .iter()
                    .filter(|effect| matches!(effect, ProbeEffect::BumpGroup { .. }))
                    .count();
                assert!(effects <= 1);
                state = next;
            }
            Err(
                TransitionError::Stale
                | TransitionError::Domain(_)
                | TransitionError::NotAllowed { .. },
            ) => {}
        }
    }
}
