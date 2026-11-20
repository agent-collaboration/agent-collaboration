//! The reply of a setting change the connection loop admitted to a run.
//!
//! The loop admits the change, so the loop claims its Lost (Lead V4,
//! admission exactness). As it hands the change to the session's actor it
//! reads the run that actor serves and wraps the caller's reply in this
//! guard. Answered, the guard sends the result. Dropped unanswered (the
//! actor ended holding it, or its mailbox closed first), it records Lost
//! for that run and no other. Only the connection task can make one: the
//! constructors are visible to it alone, and the production mailbox names
//! this type, so a setting reply in a run's mailbox is always admitted.

use std::sync::Arc;

use provider_session_machines::{ProviderSettingKind, SessionGeneration};

use crate::ExternalProviderRuntimeError;
use crate::session_cell_table::SessionCellTable;

/// The caller's end of a setting change, before any run admitted it.
pub(in crate::agent_session_client) type SettingReplySender = tokio::sync::oneshot::Sender<
    Result<crate::ProviderSettingApplication, ExternalProviderRuntimeError>,
>;

/// A setting change's reply, owed by the run it was admitted to.
pub(in crate::agent_session_client) struct AdmittedSettingReply {
    reply: Option<SettingReplySender>,
    cells: Arc<SessionCellTable>,
    session_id: String,
    generation: SessionGeneration,
    kind: ProviderSettingKind,
}

impl AdmittedSettingReply {
    /// Admits a change to the session's run as the loop hands it over: the
    /// loop moves runs itself, so the run read now is the one whose actor
    /// receives the change.
    pub(super) fn admit(
        reply: SettingReplySender,
        cells: &Arc<SessionCellTable>,
        session_id: &str,
        kind: ProviderSettingKind,
    ) -> Self {
        let generation = crate::SessionRuns::current_generation(cells.as_ref(), session_id);
        Self::for_run(reply, cells, session_id, generation, kind)
    }

    /// Admits a change to a run the loop already holds, such as the fast-off
    /// a restore queues for its own run.
    pub(super) fn for_run(
        reply: SettingReplySender,
        cells: &Arc<SessionCellTable>,
        session_id: &str,
        generation: SessionGeneration,
        kind: ProviderSettingKind,
    ) -> Self {
        Self {
            reply: Some(reply),
            cells: Arc::clone(cells),
            session_id: session_id.to_owned(),
            generation,
            kind,
        }
    }
}

/// Restore's keep-off policy runs before the newly activated actor can take
/// any user input.
pub(super) fn queue_restore_fast_off<P: crate::InteractionPort>(
    handles: &crate::provider_session_runtime_handles::ProviderSessionRuntimeHandles,
    session_id: &str,
    generation: SessionGeneration,
    actor: &tokio::sync::mpsc::UnboundedSender<
        crate::provider_session_actor::ProviderSessionCommand<P, AdmittedSettingReply>,
    >,
) {
    if !crate::session_cell_bookkeeping::keep_fast_off(&handles.session_cells, session_id)
        || !handles
            .session_settings
            .read_in(session_id, generation)
            .is_some_and(|catalog| catalog.fast_observed() == crate::FastObserved::On)
    {
        return;
    }
    let (reply, _answer) = tokio::sync::oneshot::channel();
    let reply = AdmittedSettingReply::for_run(
        reply,
        &handles.session_cells,
        session_id,
        generation,
        ProviderSettingKind::Fast,
    );
    let _queued = actor.send(
        crate::provider_session_actor::ProviderSessionCommand::SetSetting {
            request: crate::ProviderSettingRequest {
                kind: ProviderSettingKind::Fast,
                value: crate::provider_setting_application::FAST_OFF.to_owned(),
                intent: session_event_model::SettingApplicationIntent::ExplicitChange,
                expected_model: None,
            },
            reply,
        },
    );
}

impl crate::provider_session_actor::SettingReply for AdmittedSettingReply {
    fn send(
        mut self,
        result: Result<crate::ProviderSettingApplication, ExternalProviderRuntimeError>,
    ) {
        if let Some(reply) = self.reply.take() {
            let _delivered = reply.send(result);
        }
    }
}

impl Drop for AdmittedSettingReply {
    fn drop(&mut self) {
        if self.reply.take().is_some() {
            crate::session_cell_bookkeeping::apply_setting_in_run(
                &self.cells,
                &self.session_id,
                self.generation,
                self.kind,
                provider_session_machines::session_setting_machine::SessionSettingEvent::Lost,
            );
        }
    }
}

#[cfg(test)]
#[path = "admitted_setting_reply_test.rs"]
mod tests;
