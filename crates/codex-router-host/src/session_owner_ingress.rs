//! Pass Host and provider interaction facts through the session owner mailbox.

use crate::session_owner_door::SessionOwnerDoor;
use acp_client_runtime::{
    EventSinkClosed, GenerationPublication, HistoryReplayFuture, SessionEventSink,
};
use provider_session_machines::{
    SessionGeneration, session_lifecycle_machine::SessionLifecycle, session_owner::OwnerEvent,
    session_turn_machine::SessionTurn,
};
use session_event_model::{SessionEvent, SkillView};
use std::sync::{Arc, OnceLock, Weak};

#[derive(Default)]
pub(crate) struct SessionOwnerIngress {
    door: OnceLock<Weak<SessionOwnerDoor>>,
}

impl SessionOwnerIngress {
    pub(crate) fn attach(&self, door: &Arc<SessionOwnerDoor>) {
        let _attached = self.door.set(Arc::downgrade(door));
    }

    fn owner(&self, session: &str) -> Option<Arc<crate::session_owner_door::SessionOwnerAddress>> {
        self.door
            .get()?
            .upgrade()?
            .existing_provider_session(session)
    }
}

impl SessionEventSink for SessionOwnerIngress {
    fn publish_for_turn(
        &self,
        session: &str,
        generation: SessionGeneration,
        turn: provider_session_machines::session_turn_machine::TurnId,
        event: SessionEvent,
    ) -> Result<GenerationPublication, EventSinkClosed> {
        let Some(owner) = self.owner(session) else {
            return Ok(GenerationPublication::Ended);
        };
        let snapshot = owner.snapshots().borrow().clone();
        if !matches!(snapshot.lifecycle,
            SessionLifecycle::Live { generation: live } | SessionLifecycle::Closing { generation: live, .. } if live == generation
        ) || !matches!(snapshot.turn, SessionTurn::Running { turn: active } | SessionTurn::Cancelling { turn: active } if active == turn)
        {
            return Ok(GenerationPublication::Ended);
        }
        owner.report_event(OwnerEvent::Stream {
            run: generation,
            expected_turn: Some(turn),
            event,
        })?;
        Ok(GenerationPublication::Published)
    }

    fn publish_connection_auth(
        &self,
        session: &str,
        status: session_event_model::ProviderAuthStatus,
    ) -> Result<(), EventSinkClosed> {
        let Some(owner) = self.owner(session) else {
            return Ok(());
        };
        let snapshot = owner.snapshots().borrow().clone();
        let (SessionLifecycle::Live { generation }, Some(mut capabilities)) =
            (snapshot.lifecycle, snapshot.projection.capabilities)
        else {
            return Ok(());
        };
        capabilities.auth_status = status;
        owner.report_event(OwnerEvent::Stream {
            run: generation,
            expected_turn: None,
            event: SessionEvent::CapabilitiesChanged { capabilities },
        })
    }
    fn publish_skill_view(&self, session: &str, view: SkillView) -> Result<(), EventSinkClosed> {
        let Some(owner) = self.owner(session) else {
            return Ok(());
        };
        let run = self.current_generation(session);
        owner.report_event(OwnerEvent::SkillView { run, view })
    }

    fn begin_history_replay(&self, _: &str) -> HistoryReplayFuture<'_> {
        Box::pin(std::future::ready(Ok(())))
    }

    fn publish(&self, session: &str, event: SessionEvent) -> Result<(), EventSinkClosed> {
        self.publish_in(session, self.current_generation(session), event)
            .map(|_| ())
    }

    fn current_generation(&self, session: &str) -> SessionGeneration {
        let Some(owner) = self.owner(session) else {
            return SessionGeneration::UNTRACKED;
        };
        match owner.snapshots().borrow().lifecycle {
            SessionLifecycle::Live { generation }
            | SessionLifecycle::Closing { generation, .. } => generation,
            _ => SessionGeneration::UNTRACKED,
        }
    }

    fn publish_in(
        &self,
        session: &str,
        generation: SessionGeneration,
        event: SessionEvent,
    ) -> Result<GenerationPublication, EventSinkClosed> {
        let Some(owner) = self.owner(session) else {
            return Ok(GenerationPublication::Ended);
        };
        let snapshot = owner.snapshots().borrow().clone();
        if !matches!(snapshot.lifecycle,
            SessionLifecycle::Live { generation: live } | SessionLifecycle::Closing { generation: live, .. } if live == generation
        ) {
            return Ok(GenerationPublication::Ended);
        }
        let turn = match snapshot.turn {
            SessionTurn::Idle => None,
            SessionTurn::Running { turn } | SessionTurn::Cancelling { turn } => Some(turn),
        };
        owner.report_event(OwnerEvent::Stream {
            run: generation,
            expected_turn: turn,
            event,
        })?;
        Ok(GenerationPublication::Published)
    }
}
