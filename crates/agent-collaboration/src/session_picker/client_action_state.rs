//! An admitted action owns its captured source until settlement or explicit
//! detachment. Refreshes and repeated keys never create another submission.
use super::{SessionsPickerOutcome, picker_model::SessionsPickerModel};
use crate::sessions::{
    source_client_action::{CapturedClientAction, SessionClientEffect, SessionClientExecutor},
    source_ssh_protocol::{SourceReadError, SourceReply},
};
use iocraft::prelude::*;

impl SessionsPickerModel {
    pub(super) fn begin_new_client_action(
        &mut self,
        draft: &super::new_session_form::NewSessionDraft,
    ) {
        use super::new_session_client_inventory::{
            CursorCreationAvailability, NewClientInventoryState,
        };
        use super::new_session_form::{NewSessionClient, client_availability, machine_choices};
        use crate::sessions::source_ssh_protocol::{SourceManagedAction, SourceNativeClaudeAction};
        use codex_router_tui::ChoiceAvailability;
        if self.client_action.is_active()
            || self.source_action.pending.is_some()
            || self.show_help
            || client_availability(draft, draft.client) != ChoiceAvailability::Available
            || !machine_choices(self).iter().any(|choice| {
                choice.key == draft.selected_source
                    && choice.availability == ChoiceAvailability::Available
            })
        {
            return;
        }
        let NewClientInventoryState::Ready(capabilities) = &draft.client_inventory else {
            return;
        };
        let Some(directory) = draft
            .directory
            .to_str()
            .and_then(|path| path.to_owned().try_into().ok())
        else {
            return;
        };
        let effect = match draft.client {
            NewSessionClient::CodexNative => return,
            NewSessionClient::ClaudeNativeBackground => SessionClientEffect::Native {
                service_epoch: capabilities.service_epoch.clone(),
                action: SourceNativeClaudeAction::New {
                    directory,
                    arguments: draft.arguments.clone(),
                },
            },
            NewSessionClient::CursorManagedAcp => {
                let CursorCreationAvailability::Ready(binding) = &capabilities.cursor else {
                    return;
                };
                SessionClientEffect::Managed {
                    operation_id: collaboration_client::protocol::OperationId::generate(),
                    binding: binding.clone(),
                    action: SourceManagedAction::Create { directory },
                }
            }
        };
        let Some(attempt) = self.next_client_generation.checked_add(1) else {
            return;
        };
        self.next_client_generation = attempt;
        self.client_action = PickerClientAction::Queued(CapturedClientAction {
            attempt,
            source: draft.selected_source.clone(),
            service_id: capabilities.service_id.clone(),
            effect,
        });
        self.new_session = None;
        self.details_dialog = None;
    }
}

#[derive(Clone, Debug, Default)]
pub(super) enum PickerClientAction {
    #[default]
    Idle,
    Queued(CapturedClientAction),
    Running(CapturedClientAction),
    Finished {
        request: CapturedClientAction,
        result: Result<SourceReply, SourceReadError>,
        focus: ClientResultAction,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ClientResultAction {
    Close,
    CheckOperation,
}

impl PickerClientAction {
    pub(super) fn is_active(&self) -> bool {
        !matches!(self, Self::Idle)
    }
    pub(super) fn interrupted(&self) -> Option<CapturedClientAction> {
        match self {
            Self::Running(request)
                if !matches!(request.effect, SessionClientEffect::ReadManaged { .. }) =>
            {
                Some(request.clone())
            }
            _ => None,
        }
    }
}

pub(super) fn use_client_actions(
    hooks: &mut Hooks<'_, '_>,
    mut model: State<SessionsPickerModel>,
    executor: Option<SessionClientExecutor>,
) {
    let has_executor = executor.is_some();
    let submit=hooks.use_async_handler(move |request:CapturedClientAction| {
        let executor=executor.clone();
        async move {
            let Some(executor)=executor else {return};
            let result=executor(request.clone()).await;
            let current=matches!(&model.read().client_action,PickerClientAction::Running(current) if current.attempt==request.attempt);
            if current {model.write().client_action=PickerClientAction::Finished {request,result,focus:ClientResultAction::Close};}
        }
    });
    let queued = if has_executor {
        match &model.read().client_action {
            PickerClientAction::Queued(request) => Some(request.clone()),
            _ => None,
        }
    } else {
        None
    };
    if let Some(request) = queued {
        model.write().client_action = PickerClientAction::Running(request.clone());
        submit(request);
    }
}

pub(super) fn interrupt_client_action(
    model: &SessionsPickerModel,
) -> Option<SessionsPickerOutcome> {
    model
        .client_action
        .interrupted()
        .map(|request| SessionsPickerOutcome::InterruptedClientAction(Box::new(request)))
}

#[cfg(test)]
#[path = "client_action_component_tests.rs"]
mod tests;

impl SessionsPickerModel {
    pub(super) fn begin_details_client_action(
        &mut self,
        dialog: &super::details_dialog_state::SessionDetailsDialog,
        action: super::details_dialog_state::DetailsAction,
    ) -> bool {
        use super::details_client_inventory::{
            DetailsClientCapabilities, DetailsClientQuery, DetailsClientReadState,
        };
        use super::details_dialog_state::DetailsAction;
        use crate::sessions::{
            SessionPickerIdentity,
            source_ssh_protocol::{SourceManagedAction, SourceNativeClaudeAction},
        };
        if self.client_action.is_active()
            || self.source_action.pending.is_some()
            || self.show_help
            || self.new_session.is_some()
        {
            return false;
        }
        let record = dialog.record();
        let source = record
            .source_context
            .clone()
            .unwrap_or(super::PickerSourceContext::DefaultHosted);
        let DetailsClientReadState::Ready(capabilities) = &dialog.client_inventory else {
            return false;
        };
        if !capabilities.matches_query(&DetailsClientQuery {
            source: source.clone(),
            record: Box::new(record.clone()),
        }) {
            return false;
        }
        let (service_id, effect) = match (capabilities, &record.identity, action) {
            (
                DetailsClientCapabilities::Managed { binding, .. },
                SessionPickerIdentity::HostedProvider(target),
                DetailsAction::Load | DetailsAction::Resume,
            ) => {
                let operation_id = collaboration_client::protocol::OperationId::generate();
                let action = if action == DetailsAction::Load {
                    SourceManagedAction::Load {
                        target: target.clone(),
                    }
                } else {
                    SourceManagedAction::Resume {
                        target: target.clone(),
                    }
                };
                (
                    binding.endpoint.service_id.clone(),
                    SessionClientEffect::Managed {
                        operation_id,
                        binding: binding.as_ref().clone(),
                        action,
                    },
                )
            }
            (
                DetailsClientCapabilities::Native { source },
                SessionPickerIdentity::NativeClaude(target),
                DetailsAction::Resume,
            ) => {
                let Some(directory) = record
                    .cwd
                    .as_ref()
                    .and_then(|value| value.clone().try_into().ok())
                else {
                    return false;
                };
                (
                    source.service_id.clone(),
                    SessionClientEffect::Native {
                        service_epoch: source.service_epoch.clone(),
                        action: SourceNativeClaudeAction::Resume {
                            target: target.clone(),
                            directory,
                            arguments: self.request.native_arguments.clone(),
                        },
                    },
                )
            }
            _ => return false,
        };
        let Some(attempt) = self.next_client_generation.checked_add(1) else {
            return false;
        };
        self.next_client_generation = attempt;
        self.client_action = PickerClientAction::Queued(CapturedClientAction {
            attempt,
            source,
            service_id,
            effect,
        });
        self.details_dialog = None;
        true
    }
}

impl SessionsPickerModel {
    pub(super) fn begin_native_fork(
        &mut self,
        selection: super::picker_fork_confirmation::NativeForkSelection,
    ) -> bool {
        use crate::sessions::source_ssh_protocol::SourceNativeClaudeAction;
        if self.client_action.is_active() || self.source_action.pending.is_some() || self.show_help
        {
            return false;
        }
        let Some(attempt) = self.next_client_generation.checked_add(1) else {
            return false;
        };
        self.next_client_generation = attempt;
        self.client_action = PickerClientAction::Queued(CapturedClientAction {
            attempt,
            source: selection.source,
            service_id: selection.binding.service_id,
            effect: SessionClientEffect::Native {
                service_epoch: selection.binding.service_epoch,
                action: SourceNativeClaudeAction::Fork {
                    target: selection.target,
                    directory: selection.directory,
                    arguments: selection.arguments,
                },
            },
        });
        self.fork_confirmation = None;
        true
    }
}
