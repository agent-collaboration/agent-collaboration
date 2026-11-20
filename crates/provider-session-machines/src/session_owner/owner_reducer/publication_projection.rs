//! Committed presentation facts carried into a publication rebase.

use super::{OwnerRejection, OwnerResult, OwnerTransition};
use crate::SessionGeneration;
use crate::session_lifecycle_machine::{LoadAttempt, RestoreVia};
use crate::session_owner::{
    PublicationFact, RunAttempt, SessionLifecycle, SessionOwnerState, SessionProjection,
    SessionPublication,
};
use crate::session_turn_machine::{SessionTurn, TurnId};
use session_event_model::{
    CapabilityReport, PendingInteraction, PendingInteractions, SessionEvent, SessionSettings,
    SessionState, SkillView,
};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::session_owner) struct OwnerProjection {
    closed: bool,
    capabilities: Option<CapabilityReport>,
    settings: Option<SessionSettings>,
    pub(super) skills: SkillView,
    pending: BTreeMap<String, PendingInteraction>,
}

impl Default for OwnerProjection {
    fn default() -> Self {
        Self {
            closed: false,
            capabilities: None,
            settings: None,
            skills: SkillView::Unknown,
            pending: BTreeMap::new(),
        }
    }
}

impl SessionOwnerState {
    pub(super) fn project_snapshot(&self, turn: SessionTurn) -> SessionProjection {
        let projection = &self.ports.projection;
        let state = match self.lifecycle() {
            SessionLifecycle::NotLoaded { .. } if projection.closed => SessionState::Closed,
            SessionLifecycle::NotLoaded { .. } | SessionLifecycle::Loading { .. } => {
                SessionState::Unloaded
            }
            SessionLifecycle::Live { .. } | SessionLifecycle::Closing { .. } => {
                let pending =
                    PendingInteractions::new(projection.pending.values().cloned().collect());
                if let Some(pending) = pending {
                    SessionState::RequiresAction { pending }
                } else if turn == SessionTurn::Idle {
                    SessionState::Idle
                } else {
                    SessionState::Running
                }
            }
        };
        SessionProjection {
            state,
            capabilities: projection.capabilities.clone(),
            settings: projection.settings.clone(),
            skills: projection.skills.clone(),
        }
    }
}

impl OwnerTransition {
    pub(super) fn resolve_previous_turn_interactions(&mut self, run: SessionGeneration) {
        let facts = std::mem::take(&mut self.next.ports.projection.pending)
            .into_keys()
            .map(|request_id| PublicationFact::Stream {
                run,
                event: SessionEvent::InteractionResolved { request_id },
            })
            .collect::<Vec<_>>();
        if !facts.is_empty() {
            self.offer_commit(facts);
        }
    }
    pub(super) fn observe_replay(
        &mut self,
        run: RunAttempt<LoadAttempt>,
        events: Vec<SessionEvent>,
    ) -> OwnerResult {
        if self.next.lifecycle()
            != (SessionLifecycle::Loading {
                generation: run.generation,
                attempt: run.attempt,
                via: RestoreVia::Load,
            })
        {
            return Err(OwnerRejection::IgnoredLate);
        }
        self.next.ports.replay.extend(events);
        Ok(())
    }

    pub(super) fn observe_stream(
        &mut self,
        run: SessionGeneration,
        expected_turn: Option<TurnId>,
        event: SessionEvent,
    ) -> OwnerResult {
        if self.next.cell.run_generation() != Some(run)
            || !matches!(
                self.next.lifecycle(),
                SessionLifecycle::Live { .. } | SessionLifecycle::Closing { .. }
            )
            || expected_turn.is_some_and(|expected| match self.next.published_turn() {
                SessionTurn::Running { turn } | SessionTurn::Cancelling { turn } => {
                    turn != expected
                }
                SessionTurn::Idle => true,
            })
        {
            return Err(OwnerRejection::IgnoredLate);
        }
        let projection = &mut self.next.ports.projection;
        match &event {
            SessionEvent::ItemStarted { .. }
            | SessionEvent::ItemUpdated { .. }
            | SessionEvent::ItemCompleted { .. } => {}
            SessionEvent::InteractionRequested { interaction } => {
                if projection.pending.contains_key(interaction.request_id()) {
                    return Err(OwnerRejection::IgnoredLate);
                }
                projection
                    .pending
                    .insert(interaction.request_id().to_owned(), interaction.clone());
            }
            SessionEvent::InteractionResolved { request_id } => {
                if projection.pending.remove(request_id).is_none() {
                    return Err(OwnerRejection::IgnoredLate);
                }
            }
            SessionEvent::CapabilitiesChanged { capabilities } => {
                projection.capabilities = Some(capabilities.clone());
            }
            SessionEvent::SettingsChanged { settings } => {
                projection.settings = Some(settings.clone());
                self.observe_catalogue_values(settings)?;
            }
            SessionEvent::TurnStarted { .. }
            | SessionEvent::InputAccepted { .. }
            | SessionEvent::TurnEnded { .. }
            | SessionEvent::StateChanged { .. }
            | SessionEvent::ResyncRequired { .. } => return Err(OwnerRejection::IgnoredLate),
        }
        self.offer_commit(vec![PublicationFact::Stream { run, event }]);
        Ok(())
    }

    pub(super) fn observe_catalogue_values(&mut self, settings: &SessionSettings) -> OwnerResult {
        use crate::{
            ProviderSettingKind,
            session_cell_machine::SessionCellEvent,
            session_owner::{ObservedSetting, SettingEvidence},
            session_setting_machine::{SessionSetting, SessionSettingEvent, SettingValueId},
        };
        for (kind, value) in [
            (ProviderSettingKind::Mode, settings.mode.as_ref()),
            (ProviderSettingKind::Model, settings.model.as_ref()),
            (ProviderSettingKind::Effort, settings.effort.as_ref()),
        ] {
            if self.setting_fenced(kind)
                || self
                    .next
                    .cell
                    .settings
                    .get(&kind)
                    .is_some_and(|setting| !matches!(setting, SessionSetting::Current { .. }))
            {
                continue;
            }
            if let Some(value) = value {
                let value = SettingValueId(value.clone());
                self.cell_move(SessionCellEvent::Setting {
                    kind,
                    event: SessionSettingEvent::Observed {
                        value: value.clone(),
                    },
                })?;
                self.next.ports.observed.insert(
                    kind,
                    SettingEvidence {
                        value: ObservedSetting::Confirmed(value),
                        cause: None,
                    },
                );
            } else {
                self.next.ports.observed.remove(&kind);
            }
        }
        Ok(())
    }

    pub(super) fn observe_submitted_input(
        &mut self,
        input: &crate::session_owner::OwnerInput,
        item: session_event_model::SessionItem,
    ) -> OwnerResult {
        let run = input.generation;
        if self.next.cell.run_generation() != Some(run)
            || !self.next.ports.admitted_inputs.contains(&input.input)
            || !matches!(self.next.published_turn(),
                SessionTurn::Running { turn } | SessionTurn::Cancelling { turn } if turn == input.turn)
        {
            return Err(OwnerRejection::IgnoredLate);
        }
        let item_id = item.item_id.clone();
        self.offer_commit(vec![
            PublicationFact::Stream {
                run,
                event: SessionEvent::ItemStarted { item },
            },
            PublicationFact::Stream {
                run,
                event: SessionEvent::ItemCompleted { item_id },
            },
        ]);
        Ok(())
    }

    pub(super) fn observe_skill_view(
        &mut self,
        run: SessionGeneration,
        view: SkillView,
    ) -> OwnerResult {
        self.require_live(run)?;
        self.next.ports.projection.skills = view.clone();
        self.offer_commit(vec![PublicationFact::Skills { run, view }]);
        Ok(())
    }

    pub(super) fn apply_committed_projection(
        &mut self,
        fact: &SessionPublication,
        facts: &mut Vec<PublicationFact>,
    ) {
        let projection = &mut self.next.ports.projection;
        match fact {
            SessionPublication::Created(_) | SessionPublication::Activated { .. } => {
                projection.closed = false;
            }
            SessionPublication::Closed(run) | SessionPublication::Unloaded(run) => {
                for (request_id, _) in std::mem::take(&mut projection.pending) {
                    facts.push(PublicationFact::Stream {
                        run: *run,
                        event: SessionEvent::InteractionResolved { request_id },
                    });
                }
                projection.closed = matches!(fact, SessionPublication::Closed(_));
                projection.capabilities = None;
                projection.settings = None;
                projection.skills = SkillView::Unknown;
            }
            SessionPublication::TurnEnded { generation, .. } => {
                for (request_id, _) in std::mem::take(&mut projection.pending) {
                    facts.push(PublicationFact::Stream {
                        run: *generation,
                        event: SessionEvent::InteractionResolved { request_id },
                    });
                }
            }
            _ => {}
        }
    }

    pub(super) fn project_restore_setup(
        &mut self,
        capabilities: &CapabilityReport,
        settings: &SessionSettings,
    ) {
        let projection = &mut self.next.ports.projection;
        projection.capabilities = Some(capabilities.clone());
        projection.settings = Some(settings.clone());
    }
}
