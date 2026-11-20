use super::{OwnerRejection, OwnerResult, OwnerTransition};
use crate::SessionGeneration;
use crate::session_lifecycle_machine::SessionLifecycle;
use crate::session_owner::{RoutedSessionToolEvent, RoutedTool, SessionOwnerEffect};
use crate::session_turn_machine::{SessionTurn, TurnId};
use crate::tool_ownership::{
    ToolOwnershipEffect, ToolOwnershipEvent, ToolSessionPhase, reduce_tool_ownership,
};

impl OwnerTransition {
    pub(super) fn routed_tool(&mut self, event: RoutedTool) -> OwnerResult {
        match event {
            RoutedTool::Exact { run, turn, tool_id } => {
                if self.tool_phase(run)
                    != Some(ToolSessionPhase::LiveRunning {
                        generation: run,
                        turn,
                    })
                {
                    return Err(OwnerRejection::IgnoredLate);
                }
                self.apply_tool(RoutedSessionToolEvent::RoutedCallback {
                    generation: run,
                    tool_id,
                })
            }
            RoutedTool::ExactIdle { run, item_id } => {
                if self.tool_phase(run) != Some(ToolSessionPhase::LiveIdle { generation: run }) {
                    return Err(OwnerRejection::IgnoredLate);
                }
                self.apply_tool(RoutedSessionToolEvent::RetiredUpdate {
                    generation: run,
                    item_exists: self.next.ports.retained_items.contains(&item_id),
                    tool_id: item_id,
                })
            }
            RoutedTool::Observed {
                run,
                turn,
                tool_id,
                observation,
            } => {
                if let Some(turn) = turn
                    && self.tool_phase(run)
                        != Some(ToolSessionPhase::LiveRunning {
                            generation: run,
                            turn,
                        })
                {
                    return Err(OwnerRejection::IgnoredLate);
                }
                self.apply_tool(RoutedSessionToolEvent::Observed {
                    generation: run,
                    tool_id,
                    observation,
                })
            }
            RoutedTool::Replay { run, item } => {
                if !self
                    .next
                    .restore
                    .is_some_and(|pending| pending.run() == run)
                {
                    return Err(OwnerRejection::IgnoredLate);
                }
                self.next
                    .ports
                    .replay
                    .push(session_event_model::SessionEvent::ItemStarted { item });
                Ok(())
            }
            RoutedTool::ItemRetained { run, item_id } => {
                self.require_live(run)?;
                self.next.ports.retained_items.insert(item_id);
                Ok(())
            }
        }
    }

    pub(super) fn apply_tool(&mut self, event: RoutedSessionToolEvent) -> OwnerResult {
        let (expected, tool_id) = match &event {
            RoutedSessionToolEvent::Observed {
                generation,
                tool_id,
                ..
            }
            | RoutedSessionToolEvent::RetiredUpdate {
                generation,
                tool_id,
                ..
            }
            | RoutedSessionToolEvent::RoutedCallback {
                generation,
                tool_id,
            } => (*generation, tool_id.clone()),
        };
        let phase = self
            .tool_phase(expected)
            .ok_or(OwnerRejection::IgnoredLate)?;
        let observed = match event {
            RoutedSessionToolEvent::Observed { observation, .. } => {
                ToolOwnershipEvent::Observe { phase, observation }
            }
            RoutedSessionToolEvent::RetiredUpdate { item_exists, .. } => {
                ToolOwnershipEvent::ProjectRetiredUpdate { phase, item_exists }
            }
            RoutedSessionToolEvent::RoutedCallback { .. } => ToolOwnershipEvent::Callback { phase },
        };
        let (owner, effect) =
            reduce_tool_ownership(self.next.tools.get(&tool_id).copied(), observed);
        if let Some(owner) = owner {
            self.next.tools.insert(tool_id.clone(), owner);
        }
        match effect {
            ToolOwnershipEffect::None => {}
            ToolOwnershipEffect::ForwardFrame { .. }
                if matches!(phase, ToolSessionPhase::Loading { .. }) =>
            {
                // Replay publication is staged on the stamped restore attempt.
            }
            ToolOwnershipEffect::CheckRetainedItem => {
                self.release(SessionOwnerEffect::CheckRetainedTool {
                    generation: expected,
                    tool_id,
                })
            }
            ToolOwnershipEffect::ForwardFrame { generation }
            | ToolOwnershipEffect::ContinueRetainedItem { generation }
            | ToolOwnershipEffect::ResolveOwner { generation }
            | ToolOwnershipEffect::UseSoleRunningTurn { generation } => {
                self.release(SessionOwnerEffect::ForwardTool {
                    generation,
                    tool_id,
                });
            }
        }
        Ok(())
    }

    fn tool_phase(&self, expected: SessionGeneration) -> Option<ToolSessionPhase> {
        match self.next.lifecycle() {
            SessionLifecycle::Loading { generation, .. } if generation == expected => {
                Some(ToolSessionPhase::Loading { generation })
            }
            SessionLifecycle::Live { generation } if generation == expected => {
                Some(match self.next.snapshot().turn {
                    SessionTurn::Idle => ToolSessionPhase::LiveIdle { generation },
                    SessionTurn::Running { turn } | SessionTurn::Cancelling { turn } => {
                        ToolSessionPhase::LiveRunning { generation, turn }
                    }
                })
            }
            _ => None,
        }
    }

    pub(super) fn retire_turn_tools(&mut self, generation: SessionGeneration, turn: TurnId) {
        self.retire_tools(ToolOwnershipEvent::TurnEnded { generation, turn });
    }

    pub(super) fn retire_run_tools(&mut self, generation: SessionGeneration) {
        self.retire_tools(ToolOwnershipEvent::RunRetired { generation });
    }

    fn retire_tools(&mut self, event: ToolOwnershipEvent) {
        for owner in self.next.tools.values_mut() {
            if let (Some(next), _) = reduce_tool_ownership(Some(*owner), event) {
                *owner = next;
            }
        }
    }
}
