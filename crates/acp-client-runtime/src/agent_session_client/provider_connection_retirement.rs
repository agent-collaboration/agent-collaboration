//! What a provider connection's end publishes for its sessions: waiting
//! queued messages end not sent through the Host's queue sink, running turns
//! end Lost, and every session it knew reads Unloaded.

use super::{
    Arc, CursorPlanItems, InteractionPort, ProviderConnectionActivity, ProviderKnownSessions,
    SessionEventSink,
};
use crate::session_cell_table::SessionCellTable;

pub(super) struct ConnectionRetirement<P: InteractionPort> {
    pub(super) session_cells: Arc<SessionCellTable>,
    pub(super) interaction_port: Arc<P>,
    pub(super) activity: Arc<ProviderConnectionActivity>,
    pub(super) todos: Arc<CursorPlanItems>,
    pub(super) known_sessions: ProviderKnownSessions,
    pub(super) event_sink: Arc<dyn SessionEventSink>,
}

impl<P: InteractionPort> ConnectionRetirement<P> {
    pub(super) async fn retire(self) {
        // Messages still waiting in this connection's lines end not sent,
        // "provider connection closed", through the Host's queue sink.
        for session_id in self.session_cells.session_ids() {
            let _ended = self.session_cells.transition(
                &session_id,
                crate::session_cell_machine::SessionCellEvent::ConnectionClosed,
                crate::session_cell_table::EntryEffectRoute::Sink,
            );
        }
        self.interaction_port.cancel_retired().await;
        for (session_id, turn_id) in self.activity.drain_running_turns() {
            let _result = self.event_sink.publish(
                &session_id,
                session_event_model::SessionEvent::TurnEnded {
                    turn_id,
                    outcome: session_event_model::TurnOutcome::Lost {
                        reason: session_event_model::TurnLostReason::ProviderRetired,
                    },
                },
            );
            self.todos.forget_session(&session_id);
        }
        for session_id in self.known_sessions.snapshot().await {
            let _result = self.event_sink.publish(
                &session_id,
                session_event_model::SessionEvent::StateChanged {
                    state: session_event_model::SessionState::Unloaded,
                },
            );
        }
    }
}
