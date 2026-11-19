//! Ownership of a tool ID within one provider session on a connection.

use crate::SessionGeneration;
use crate::session_turn_machine::TurnId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolOrigin {
    Live,
    Replay,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolOwnership {
    Owned {
        generation: SessionGeneration,
        origin: ToolOrigin,
        turn: Option<TurnId>,
    },
    Retired {
        generation: SessionGeneration,
    },
}

impl ToolOwnership {
    /// Only owned records can identify a callback candidate.
    #[must_use]
    pub fn owned_generation(self) -> Option<SessionGeneration> {
        match self {
            Self::Owned { generation, .. } => Some(generation),
            Self::Retired { .. } => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolSessionPhase {
    Loading {
        generation: SessionGeneration,
    },
    LiveIdle {
        generation: SessionGeneration,
    },
    LiveRunning {
        generation: SessionGeneration,
        turn: TurnId,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolObservation {
    Announcement,
    Continuation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolOwnershipEvent {
    Observe {
        phase: ToolSessionPhase,
        observation: ToolObservation,
    },
    Callback {
        phase: ToolSessionPhase,
    },
    ProjectRetiredUpdate {
        phase: ToolSessionPhase,
        item_exists: bool,
    },
    TurnEnded {
        generation: SessionGeneration,
        turn: TurnId,
    },
    RunRetired {
        generation: SessionGeneration,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolOwnershipEffect {
    None,
    ForwardFrame {
        generation: SessionGeneration,
    },
    /// The frame may reach the actor, which owns knowledge of its items.
    CheckRetainedItem,
    ContinueRetainedItem {
        generation: SessionGeneration,
    },
    ResolveOwner {
        generation: SessionGeneration,
    },
    UseSoleRunningTurn {
        generation: SessionGeneration,
    },
}

/// None means the connection has no observation for this key. Retirement
/// keeps the ID known until a later replay refreshes it or the connection ends.
#[must_use]
pub fn reduce_tool_ownership(
    owner: Option<ToolOwnership>,
    event: ToolOwnershipEvent,
) -> (Option<ToolOwnership>, ToolOwnershipEffect) {
    use ToolOwnership::{Owned, Retired};
    use ToolOwnershipEffect::{ForwardFrame, ResolveOwner, UseSoleRunningTurn};
    use ToolSessionPhase::{LiveIdle, LiveRunning, Loading};

    match (owner, event) {
        (
            previous,
            ToolOwnershipEvent::Observe {
                phase: Loading { generation },
                ..
            },
        ) if match previous {
            None => true,
            Some(Owned {
                generation: prior, ..
            }) => prior.number() <= generation.number(),
            Some(Retired { generation: prior }) => prior.number() < generation.number(),
        } =>
        {
            (
                Some(Owned {
                    generation,
                    origin: ToolOrigin::Replay,
                    turn: None,
                }),
                ForwardFrame { generation },
            )
        }
        (
            Some(Retired { generation: prior }),
            ToolOwnershipEvent::Observe {
                phase: LiveIdle { generation },
                observation: ToolObservation::Continuation,
            },
        ) if prior == generation => (owner, ToolOwnershipEffect::CheckRetainedItem),
        (
            Some(Retired { generation: prior }),
            ToolOwnershipEvent::ProjectRetiredUpdate {
                phase: LiveIdle { generation },
                item_exists: true,
            },
        ) if prior == generation => (
            owner,
            ToolOwnershipEffect::ContinueRetainedItem { generation },
        ),
        (
            previous,
            ToolOwnershipEvent::Observe {
                phase: LiveIdle { generation },
                ..
            },
        ) if previous.is_none()
            || matches!(previous, Some(Owned { generation: prior, turn: None, .. }) if prior == generation) =>
        {
            (
                previous.or(Some(Owned {
                    generation,
                    origin: ToolOrigin::Live,
                    turn: None,
                })),
                ForwardFrame { generation },
            )
        }
        (
            previous,
            ToolOwnershipEvent::Observe {
                phase: LiveRunning { generation, turn },
                observation,
            },
        ) if previous.is_none()
            || matches!(previous, Some(Owned { generation: prior, turn: Some(claimed), .. }) if prior == generation && claimed == turn)
            || matches!((previous, observation), (Some(Owned { generation: prior, turn: None, .. }), ToolObservation::Continuation) if prior == generation) =>
        {
            (
                previous.or(Some(Owned {
                    generation,
                    origin: ToolOrigin::Live,
                    turn: Some(turn),
                })),
                ForwardFrame { generation },
            )
        }
        (
            Some(Owned {
                generation: prior,
                turn: None,
                ..
            }),
            ToolOwnershipEvent::Callback {
                phase: LiveIdle { generation },
            },
        ) if prior == generation => (owner, ResolveOwner { generation }),
        (
            Some(Owned {
                generation: prior,
                turn: claimed,
                ..
            }),
            ToolOwnershipEvent::Callback {
                phase: LiveRunning { generation, turn },
            },
        ) if prior == generation && claimed.is_none_or(|claimed| claimed == turn) => {
            (owner, ResolveOwner { generation })
        }
        (
            None,
            ToolOwnershipEvent::Callback {
                phase: LiveRunning { generation, .. },
            },
        ) => (None, UseSoleRunningTurn { generation }),
        (
            Some(Owned {
                generation: prior,
                turn: claimed,
                ..
            }),
            ToolOwnershipEvent::TurnEnded { generation, turn },
        ) if prior == generation && claimed.is_none_or(|claimed| claimed == turn) => {
            (Some(Retired { generation }), ToolOwnershipEffect::None)
        }
        (
            Some(Owned {
                generation: prior, ..
            }),
            ToolOwnershipEvent::RunRetired { generation },
        ) if prior == generation => (Some(Retired { generation }), ToolOwnershipEffect::None),
        _ => (owner, ToolOwnershipEffect::None),
    }
}

#[cfg(test)]
#[path = "tool_ownership_test.rs"]
mod tests;
