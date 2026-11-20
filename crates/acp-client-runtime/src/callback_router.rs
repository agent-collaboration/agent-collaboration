//! Connection-local callback identity and turn eligibility.

use crate::transport_run::TransportRun;
use provider_session_machines::{
    session_turn_machine::TurnId,
    tool_ownership::{
        ToolObservation, ToolOwnership, ToolOwnershipEffect, ToolOwnershipEvent, ToolSessionPhase,
        reduce_tool_ownership,
    },
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CallbackKind {
    Continuation,
    Request,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CallbackResolution {
    Exact {
        run: TransportRun,
        turn: TurnId,
    },
    ExactIdle {
        run: TransportRun,
        item_id: String,
    },
    Ambiguous,
    Retired,
    NeverObserved {
        sole_eligible: Option<(TransportRun, TurnId)>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Membership {
    Provisional(TurnId),
    Eligible(TurnId),
    ProviderStarted(TurnId),
}

impl Membership {
    const fn eligible(self) -> Option<TurnId> {
        match self {
            Self::Eligible(turn) | Self::ProviderStarted(turn) => Some(turn),
            Self::Provisional(_) => None,
        }
    }
}

#[derive(Default)]
struct RouteTools {
    live: bool,
    turn: Option<Membership>,
    observed: BTreeSet<String>,
}

struct ToolClaim {
    run: TransportRun,
    ownership: ToolOwnership,
}

#[derive(Default)]
pub(crate) struct CallbackRouter {
    routes: BTreeMap<TransportRun, RouteTools>,
    claims: BTreeMap<String, BTreeMap<String, ToolClaim>>,
}

impl CallbackRouter {
    pub(crate) fn open(&mut self, run: TransportRun) {
        self.routes.entry(run).or_default();
    }

    pub(crate) fn activate(&mut self, run: TransportRun) {
        if let Some(route) = self.routes.get_mut(&run) {
            route.live = true;
        }
    }

    pub(crate) fn current_turn(&self, run: TransportRun) -> Option<TurnId> {
        self.routes.get(&run)?.turn?.eligible()
    }

    pub(crate) fn provider_started_turn(&self, run: TransportRun) -> Option<TurnId> {
        match self.routes.get(&run)?.turn? {
            Membership::ProviderStarted(turn) => Some(turn),
            _ => None,
        }
    }

    pub(crate) fn allows_explicit_tool_request(
        &self,
        session: &str,
        run: TransportRun,
        tool: &str,
    ) -> bool {
        let Some(turn) = self.current_turn(run) else {
            return false;
        };
        self.claims
            .get(tool)
            .and_then(|claims| claims.get(session))
            .is_none_or(|claim| {
                claim.run == run
                    && matches!(claim.ownership,
                        ToolOwnership::Owned { turn: expected, .. }
                        if expected.is_none_or(|expected| expected == turn)
                    )
            })
    }

    pub(crate) fn begin_turn(&mut self, run: TransportRun, turn: TurnId) {
        if let Some(route) = self.routes.get_mut(&run)
            && route.live
            && route.turn.is_none()
        {
            route.turn = Some(Membership::Provisional(turn));
        }
    }

    pub(crate) fn can_take_new_turn(&self, run: TransportRun, turn: TurnId) -> bool {
        self.routes.get(&run).and_then(|route| route.turn) == Some(Membership::Provisional(turn))
    }

    pub(crate) fn take_new_turn(&mut self, run: TransportRun, turn: TurnId) {
        if let Some(route) = self.routes.get_mut(&run) {
            route.turn = Some(Membership::Eligible(turn));
        }
    }

    pub(crate) fn observe_eligible_turn(&mut self, run: TransportRun, turn: TurnId) {
        if let Some(route) = self.routes.get_mut(&run)
            && route.live
            && route.turn.is_none()
        {
            route.turn = Some(Membership::ProviderStarted(turn));
        }
    }

    pub(crate) fn revoke_new_turn(&mut self, run: TransportRun, turn: TurnId) {
        if let Some(route) = self.routes.get_mut(&run)
            && route.turn == Some(Membership::Provisional(turn))
        {
            route.turn = None;
        }
    }

    pub(crate) fn end_turn(&mut self, session: &str, run: TransportRun, turn: TurnId) {
        let Some(route) = self.routes.get_mut(&run) else {
            return;
        };
        if route.turn.and_then(Membership::eligible) != Some(turn) {
            return;
        }
        route.turn = None;
        update_claims(
            &mut route.observed,
            &mut self.claims,
            session,
            run,
            ToolOwnershipEvent::TurnEnded {
                generation: run.generation,
                turn,
            },
        );
    }

    pub(crate) fn retire(&mut self, session: &str, run: TransportRun) {
        if let Some(mut route) = self.routes.remove(&run) {
            update_claims(
                &mut route.observed,
                &mut self.claims,
                session,
                run,
                ToolOwnershipEvent::RunRetired {
                    generation: run.generation,
                },
            );
        }
    }

    pub(crate) fn observe(
        &mut self,
        session: &str,
        run: TransportRun,
        tool_id: &str,
        observation: ToolObservation,
    ) -> ToolOwnershipEffect {
        let Some(route) = self.routes.get_mut(&run) else {
            return ToolOwnershipEffect::None;
        };
        let phase = match (route.live, route.turn) {
            (false, _) => ToolSessionPhase::Loading {
                generation: run.generation,
            },
            (true, Some(Membership::Eligible(turn) | Membership::ProviderStarted(turn))) => {
                ToolSessionPhase::LiveRunning {
                    generation: run.generation,
                    turn,
                }
            }
            (true, _) => ToolSessionPhase::LiveIdle {
                generation: run.generation,
            },
        };
        let previous = self
            .claims
            .get(tool_id)
            .and_then(|claims| claims.get(session));
        // A new incarnation starts its generation counter again. Only replay
        // can refresh a claim whose previous incarnation is known to this connection.
        let previous = match previous {
            Some(claim) if claim.run.incarnation != run.incarnation => {
                if !matches!(phase, ToolSessionPhase::Loading { .. }) {
                    return ToolOwnershipEffect::None;
                }
                None
            }
            other => other.map(|claim| claim.ownership),
        };
        let (ownership, effect) =
            reduce_tool_ownership(previous, ToolOwnershipEvent::Observe { phase, observation });
        if let Some(ownership) = ownership {
            // Claims rejected by the reducer must keep their original run stamp.
            if matches!(effect, ToolOwnershipEffect::ForwardFrame { .. }) {
                self.claims
                    .entry(tool_id.to_owned())
                    .or_default()
                    .insert(session.to_owned(), ToolClaim { run, ownership });
                route.observed.insert(tool_id.to_owned());
            }
        }
        effect
    }

    /// A never-observed ID can use exactly one eligible turn. Known IDs are
    /// resolved from retained claims even when only one session is active.
    pub(crate) fn resolve(&self, tool_id: &str, kind: CallbackKind) -> CallbackResolution {
        let Some(claims) = self.claims.get(tool_id) else {
            let mut turns = self
                .routes
                .iter()
                .filter_map(|(&run, route)| Some((run, route.turn?.eligible()?)));
            let sole_eligible = match (turns.next(), turns.next()) {
                (Some(candidate), None) => Some(candidate),
                _ => None,
            };
            return CallbackResolution::NeverObserved { sole_eligible };
        };
        if claims.len() > 1 {
            return CallbackResolution::Ambiguous;
        }
        let Some(claim) = claims.values().next() else {
            return CallbackResolution::NeverObserved {
                sole_eligible: None,
            };
        };
        let Some(route) = self.routes.get(&claim.run).filter(|route| route.live) else {
            return CallbackResolution::Retired;
        };
        let turn = self.current_turn(claim.run);
        match (claim.ownership, turn) {
            (ToolOwnership::Owned { turn: expected, .. }, Some(turn))
                if expected.is_none_or(|expected| expected == turn) =>
            {
                CallbackResolution::Exact {
                    run: claim.run,
                    turn,
                }
            }
            (ToolOwnership::Owned { turn: None, .. } | ToolOwnership::Retired { .. }, None)
                if route.turn.is_none() && kind == CallbackKind::Continuation =>
            {
                CallbackResolution::ExactIdle {
                    run: claim.run,
                    item_id: tool_id.to_owned(),
                }
            }
            _ => CallbackResolution::Retired,
        }
    }
}

fn update_claims(
    observed: &mut BTreeSet<String>,
    claims: &mut BTreeMap<String, BTreeMap<String, ToolClaim>>,
    session: &str,
    run: TransportRun,
    event: ToolOwnershipEvent,
) {
    observed.retain(|tool_id| {
        let Some(claim) = claims
            .get_mut(tool_id)
            .and_then(|owners| owners.get_mut(session))
            .filter(|claim| claim.run == run)
        else {
            return false;
        };
        if let (Some(ownership), _) = reduce_tool_ownership(Some(claim.ownership), event) {
            claim.ownership = ownership;
        }
        claim.ownership.owned_generation().is_some()
    });
}
