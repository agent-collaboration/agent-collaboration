use super::*;
use hegel::generators as gs;
use state_machine_core::check_state_machine_transitions;

fn events(keys: [Uuid; 2]) -> Vec<CustodyEvent> {
    let [first, second] = keys;
    vec![
        CustodyEvent::Reserve,
        CustodyEvent::HostWriteBegun,
        CustodyEvent::HostWriteEnded(CustodyEnd::NoWrite),
        CustodyEvent::HostWriteEnded(CustodyEnd::Failed),
        CustodyEvent::LaneOffered(first),
        CustodyEvent::LaneOffered(second),
        CustodyEvent::LaneReturned {
            ticket: first,
            reason: CustodyEnd::Written,
        },
        CustodyEvent::LaneReturned {
            ticket: second,
            reason: CustodyEnd::Failed,
        },
        CustodyEvent::ReleasedAtShutdown(Custodian::Host),
        CustodyEvent::ReleasedAtShutdown(Custodian::Lane(first)),
        CustodyEvent::ReleasedAtShutdown(Custodian::Lane(second)),
        CustodyEvent::DropReservation,
        CustodyEvent::RepairTaken(first),
        CustodyEvent::RepairTaken(second),
        CustodyEvent::RepairReleased(first),
        CustodyEvent::RepairReleased(second),
    ]
}

#[test]
fn custody_transitions_cover_the_declared_table() {
    let first = Uuid::now_v7();
    let second = Uuid::now_v7();
    let actions = events([first, second]);
    let mut states = vec![
        OperationCustody::Vacant,
        OperationCustody::Ended(CustodyEnd::Written),
        OperationCustody::Repairing {
            ordinary_end: CustodyEnd::Written,
            authority: first,
        },
    ];
    for host in [
        HostWriter::Reserved,
        HostWriter::Writing,
        HostWriter::Ended(CustodyEnd::NoWrite),
    ] {
        for lanes in [
            BTreeSet::new(),
            BTreeSet::from([first]),
            BTreeSet::from([first, second]),
        ] {
            if lanes.is_empty() && matches!(host, HostWriter::Ended(_)) {
                continue;
            }
            for repair in [None, Some(first)] {
                states.push(OperationCustody::Held(HeldCustody {
                    host,
                    lanes: lanes.clone(),
                    last_lane_end: None,
                    repair,
                }));
            }
        }
    }
    let cases = states
        .into_iter()
        .flat_map(|state| actions.iter().map(move |&event| (state.clone(), event)));
    assert_eq!(
        check_state_machine_transitions::<OperationCustodyMachine>(cases),
        Ok(())
    );
}

#[derive(Default)]
struct Obligations {
    reserved: bool,
    host_open: bool,
    host_started: bool,
    tickets: Vec<Uuid>,
    repair: Option<Uuid>,
}

impl Obligations {
    fn ordinary_open(&self) -> bool {
        self.host_open || !self.tickets.is_empty()
    }

    fn accept(&mut self, event: CustodyEvent) -> bool {
        match event {
            CustodyEvent::Reserve if !self.reserved => {
                self.reserved = true;
                self.host_open = true;
            }
            CustodyEvent::HostWriteBegun if self.host_open && !self.host_started => {
                self.host_started = true
            }
            CustodyEvent::HostWriteEnded(_)
            | CustodyEvent::ReleasedAtShutdown(Custodian::Host)
            | CustodyEvent::DropReservation
                if self.host_open =>
            {
                self.host_open = false
            }
            CustodyEvent::LaneOffered(ticket)
                if self.ordinary_open() && !self.tickets.contains(&ticket) =>
            {
                self.tickets.push(ticket)
            }
            CustodyEvent::LaneReturned { ticket, .. }
            | CustodyEvent::ReleasedAtShutdown(Custodian::Lane(ticket)) => {
                let Some(index) = self.tickets.iter().position(|&held| held == ticket) else {
                    return false;
                };
                self.tickets.remove(index);
            }
            CustodyEvent::RepairTaken(authority) if self.reserved && self.repair.is_none() => {
                self.repair = Some(authority)
            }
            CustodyEvent::RepairReleased(authority) if self.repair == Some(authority) => {
                self.repair = None
            }
            _ => return false,
        }
        true
    }
}

#[hegel::test(seed = Some(2911803), test_cases = 200)]
fn custody_sequence_requires_exact_receipts_and_refuses_illegal_events(case: hegel::TestCase) {
    let actions = events([Uuid::now_v7(), Uuid::now_v7()]);
    let mut state = OperationCustody::Vacant;
    let mut expected = Obligations::default();
    let steps = case.draw(gs::integers::<usize>().min_value(1).max_value(60));
    for _ in 0..steps {
        let event = actions[case.draw(gs::integers::<usize>().max_value(actions.len() - 1))];
        let before = state.clone();
        let was_open = expected.ordinary_open();
        let accepted = expected.accept(event);
        match OperationCustodyMachine::transition(&state, event) {
            Ok((next, effects)) => {
                assert!(
                    accepted,
                    "illegal event was silently accepted: {before:?} {event:?}"
                );
                assert_eq!(
                    effects.contains(&CustodyEffect::OrdinaryCustodyEnded),
                    was_open && !expected.ordinary_open()
                );
                state = next;
            }
            Err(TransitionError::Domain(_)) => {
                assert!(!accepted, "legal event refused: {before:?} {event:?}");
                assert_eq!(state, before);
            }
            Err(error) => panic!("missing typed custody refusal: {before:?} {event:?} {error:?}"),
        }
        assert_eq!(state.holds_custody(), expected.ordinary_open());
        assert_eq!(
            state.is_terminal(),
            expected.reserved && !expected.ordinary_open() && expected.repair.is_none()
        );
        assert_eq!(
            matches!(state, OperationCustody::Repairing { .. }),
            !expected.ordinary_open() && expected.repair.is_some()
        );
    }
}
