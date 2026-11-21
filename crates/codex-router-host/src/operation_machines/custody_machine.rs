//! Custody tracks who can still write an operation’s row. A store-guarded
//! repair has separate authority and cannot reopen ordinary custody.

use state_machine_core::{
    EventTag, RefusalTag, StateMachine, StateMachineResult, StateTag, Transition, TransitionError,
};
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CustodyEnd {
    Written,
    Failed,
    NoWrite,
    Dropped,
    Released,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HostWriter {
    Reserved,
    Writing,
    Ended(CustodyEnd),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HeldCustody {
    host: HostWriter,
    lanes: BTreeSet<Uuid>,
    last_lane_end: Option<CustodyEnd>,
    repair: Option<Uuid>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum OperationCustody {
    Vacant,
    Held(HeldCustody),
    Ended(CustodyEnd),
    Repairing {
        ordinary_end: CustodyEnd,
        authority: Uuid,
    },
}

impl OperationCustody {
    pub(crate) fn holds_custody(&self) -> bool {
        matches!(self, Self::Held(_))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Custodian {
    Host,
    Lane(Uuid),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CustodyEvent {
    Reserve,
    HostWriteBegun,
    HostWriteEnded(CustodyEnd),
    LaneOffered(Uuid),
    LaneReturned { ticket: Uuid, reason: CustodyEnd },
    ReleasedAtShutdown(Custodian),
    DropReservation,
    RepairTaken(Uuid),
    RepairReleased(Uuid),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CustodyEffect {
    OrdinaryCustodyEnded,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CustodyRefusal {
    WrongPhase,
    HostAlreadyStarted,
    HostAlreadyEnded,
    DuplicateLaneOffer,
    UnmatchedLaneReceipt,
    RepairAlreadyHeld,
    UnmatchedRepair,
}

impl StateTag for OperationCustody {
    fn state_tag(&self) -> &'static str {
        match self {
            Self::Vacant => "vacant",
            Self::Held(_) => "held",
            Self::Ended(_) => "ended",
            Self::Repairing { .. } => "repairing",
        }
    }

    fn is_terminal(&self) -> bool {
        matches!(self, Self::Ended(_))
    }
}

impl EventTag for CustodyEvent {
    fn event_tag(&self) -> &'static str {
        match self {
            Self::Reserve => "reserve",
            Self::HostWriteBegun => "host_write_begun",
            Self::HostWriteEnded(_) => "host_write_ended",
            Self::LaneOffered(_) => "lane_offered",
            Self::LaneReturned { .. } => "lane_returned",
            Self::ReleasedAtShutdown(_) => "released_at_shutdown",
            Self::DropReservation => "drop_reservation",
            Self::RepairTaken(_) => "repair_taken",
            Self::RepairReleased(_) => "repair_released",
        }
    }
}

impl RefusalTag for CustodyRefusal {
    fn refusal_tag(&self) -> &'static str {
        match self {
            Self::WrongPhase => "wrong_phase",
            Self::HostAlreadyStarted => "host_already_started",
            Self::HostAlreadyEnded => "host_already_ended",
            Self::DuplicateLaneOffer => "duplicate_lane_offer",
            Self::UnmatchedLaneReceipt => "unmatched_lane_receipt",
            Self::RepairAlreadyHeld => "repair_already_held",
            Self::UnmatchedRepair => "unmatched_repair",
        }
    }
}

const fn edge(
    from: &'static str,
    event: &'static str,
    to: &'static str,
    terminal: bool,
) -> Transition {
    Transition {
        from,
        event,
        to,
        terminal,
    }
}

const TRANSITIONS: &[Transition] = &[
    edge("vacant", "reserve", "held", false),
    edge("held", "host_write_begun", "held", false),
    edge("held", "host_write_ended", "held", false),
    edge("held", "host_write_ended", "ended", true),
    edge("held", "host_write_ended", "repairing", false),
    edge("held", "lane_offered", "held", false),
    edge("held", "lane_returned", "held", false),
    edge("held", "lane_returned", "ended", true),
    edge("held", "lane_returned", "repairing", false),
    edge("held", "released_at_shutdown", "held", false),
    edge("held", "released_at_shutdown", "ended", true),
    edge("held", "released_at_shutdown", "repairing", false),
    edge("held", "drop_reservation", "held", false),
    edge("held", "drop_reservation", "ended", true),
    edge("held", "drop_reservation", "repairing", false),
    edge("held", "repair_taken", "held", false),
    edge("held", "repair_released", "held", false),
    edge("ended", "repair_taken", "repairing", false),
    edge("repairing", "repair_released", "ended", true),
];

pub(crate) struct OperationCustodyMachine;

impl OperationCustodyMachine {
    pub(crate) fn reserved() -> OperationCustody {
        OperationCustody::Held(HeldCustody {
            host: HostWriter::Reserved,
            lanes: BTreeSet::new(),
            last_lane_end: None,
            repair: None,
        })
    }

    fn writer_returned(
        state: &OperationCustody,
        writer: Custodian,
        reason: CustodyEnd,
    ) -> StateMachineResult<Self> {
        let OperationCustody::Held(mut held) = state.clone() else {
            return Err(TransitionError::Domain(CustodyRefusal::WrongPhase));
        };
        match writer {
            Custodian::Host => {
                if matches!(held.host, HostWriter::Ended(_)) {
                    return Err(TransitionError::Domain(CustodyRefusal::HostAlreadyEnded));
                }
                held.host = HostWriter::Ended(reason);
            }
            Custodian::Lane(ticket) => {
                if !held.lanes.remove(&ticket) {
                    return Err(TransitionError::Domain(
                        CustodyRefusal::UnmatchedLaneReceipt,
                    ));
                }
                held.last_lane_end = Some(reason);
            }
        }
        if let HostWriter::Ended(host_end) = held.host
            && held.lanes.is_empty()
        {
            let end = if host_end == CustodyEnd::NoWrite {
                held.last_lane_end.unwrap_or(host_end)
            } else {
                host_end
            };
            let next = held
                .repair
                .map_or(OperationCustody::Ended(end), |authority| {
                    OperationCustody::Repairing {
                        ordinary_end: end,
                        authority,
                    }
                });
            Ok((next, vec![CustodyEffect::OrdinaryCustodyEnded]))
        } else {
            Ok((OperationCustody::Held(held), Vec::new()))
        }
    }
}

impl StateMachine for OperationCustodyMachine {
    type State = OperationCustody;
    type Event = CustodyEvent;
    type Effect = CustodyEffect;
    type Error = CustodyRefusal;

    fn transition(state: &OperationCustody, event: CustodyEvent) -> StateMachineResult<Self> {
        use CustodyEvent as V;
        use CustodyRefusal as R;
        use OperationCustody as S;
        let refuse = |reason| Err(TransitionError::Domain(reason));
        match event {
            V::Reserve if state == &S::Vacant => Ok((Self::reserved(), Vec::new())),
            V::Reserve => refuse(R::WrongPhase),
            V::HostWriteBegun => {
                let S::Held(mut held) = state.clone() else {
                    return refuse(R::WrongPhase);
                };
                match held.host {
                    HostWriter::Reserved => held.host = HostWriter::Writing,
                    HostWriter::Writing => return refuse(R::HostAlreadyStarted),
                    HostWriter::Ended(_) => return refuse(R::HostAlreadyEnded),
                }
                Ok((S::Held(held), Vec::new()))
            }
            V::HostWriteEnded(reason) => Self::writer_returned(state, Custodian::Host, reason),
            V::LaneOffered(ticket) => {
                let S::Held(mut held) = state.clone() else {
                    return refuse(R::WrongPhase);
                };
                if !held.lanes.insert(ticket) {
                    return refuse(R::DuplicateLaneOffer);
                }
                Ok((S::Held(held), Vec::new()))
            }
            V::LaneReturned { ticket, reason } => {
                Self::writer_returned(state, Custodian::Lane(ticket), reason)
            }
            V::ReleasedAtShutdown(writer) => {
                Self::writer_returned(state, writer, CustodyEnd::Released)
            }
            V::DropReservation => {
                Self::writer_returned(state, Custodian::Host, CustodyEnd::Dropped)
            }
            V::RepairTaken(authority) => match state {
                S::Held(held) if held.repair.is_none() => {
                    let mut held = held.clone();
                    held.repair = Some(authority);
                    Ok((S::Held(held), Vec::new()))
                }
                S::Ended(end) => Ok((
                    S::Repairing {
                        ordinary_end: *end,
                        authority,
                    },
                    Vec::new(),
                )),
                S::Held(_) | S::Repairing { .. } => refuse(R::RepairAlreadyHeld),
                S::Vacant => refuse(R::WrongPhase),
            },
            V::RepairReleased(authority) => match state {
                S::Held(held) if held.repair == Some(authority) => {
                    let mut held = held.clone();
                    held.repair = None;
                    Ok((S::Held(held), Vec::new()))
                }
                S::Repairing {
                    ordinary_end,
                    authority: expected,
                } if *expected == authority => Ok((S::Ended(*ordinary_end), Vec::new())),
                S::Held(_) | S::Repairing { .. } => refuse(R::UnmatchedRepair),
                S::Vacant | S::Ended(_) => refuse(R::WrongPhase),
            },
        }
    }

    fn transitions() -> &'static [Transition] {
        TRANSITIONS
    }
}

#[cfg(test)]
#[path = "custody_machine_test.rs"]
mod tests;
