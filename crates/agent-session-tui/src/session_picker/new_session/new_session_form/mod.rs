//! One invocation-local New draft; existing source preparation retains launch authority.
use crate::session_picker::PickerSourceContext;
use crate::session_picker::SessionsPickerOutcome;
use crate::session_picker::screen::picker_choice::PickerChoice;
use crate::session_picker::screen::picker_model::SessionsPickerModel;
use agent_collaboration_tui::actions::Availability;
use agent_session_inventory::sessions::RouterRegistryRead;
use agent_session_inventory::sessions::router_connection_registry::RouterConnectionTransport;
#[cfg(test)]
use iocraft::prelude::*;
use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum NewSessionClient {
    #[default]
    CodexNative,
    ClaudeNativeForeground,
    ClaudeNativeBackground,
    CursorManagedAcp,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NewSessionDraft {
    pub(crate) generation: u64,
    pub(crate) client_inventory:
        crate::session_picker::new_session::new_session_client_inventory::NewClientInventoryState,
    pub(crate) client: NewSessionClient,
    pub(crate) selected_source: PickerSourceContext,
    pub(crate) directory: PathBuf,
    pub(crate) arguments: Vec<OsString>,
}

impl NewSessionDraft {
    fn capture(model: &SessionsPickerModel) -> Self {
        let source = match &model.machine_controls.filter {
            crate::session_picker::source::picker_machine_controls::PickerMachineFilter::Single { source } => {
                source.as_ref().clone()
            }
            _ => match model.request.machine_mode {
                crate::session_picker::PickerMachineSourceMode::HostedDefault => PickerSourceContext::DefaultHosted,
                crate::session_picker::PickerMachineSourceMode::LocalCodex => PickerSourceContext::LocalCodex,
            },
        };
        let directory = source_directory(&source, &model.request.current_dir);
        Self {
            generation: model.next_client_generation,
            client_inventory:
                crate::session_picker::new_session::new_session_client_inventory::NewClientInventoryState::Unobserved,
            client: NewSessionClient::CodexNative,
            selected_source: source,
            directory,
            arguments: model.request.native_arguments.clone(),
        }
    }
}

fn source_directory(source: &PickerSourceContext, local: &std::path::Path) -> PathBuf {
    match source {
        PickerSourceContext::ConfiguredHosted(profile) => profile
            .default_remote_cwd
            .as_ref()
            .map(|cwd| PathBuf::from(cwd.as_str()))
            .unwrap_or_default(),
        PickerSourceContext::DefaultHosted | PickerSourceContext::LocalCodex => local.to_owned(),
    }
}
fn source_label(source: &PickerSourceContext) -> String {
    match source {
        PickerSourceContext::ConfiguredHosted(profile) => profile.name.as_str().to_owned(),
        PickerSourceContext::DefaultHosted => "This machine".to_owned(),
        PickerSourceContext::LocalCodex => "Local Codex".to_owned(),
    }
}

pub(crate) fn machine_choices(
    model: &SessionsPickerModel,
) -> Vec<PickerChoice<PickerSourceContext>> {
    let source = match model.request.machine_mode {
        crate::session_picker::PickerMachineSourceMode::HostedDefault => {
            PickerSourceContext::DefaultHosted
        }
        crate::session_picker::PickerMachineSourceMode::LocalCodex => {
            PickerSourceContext::LocalCodex
        }
    };
    let mut choices = vec![PickerChoice::available(
        source.clone(),
        source_label(&source),
    )];
    if model.request.machine_mode == crate::session_picker::PickerMachineSourceMode::HostedDefault
        && let RouterRegistryRead::Ready(registry) = &model.request.router_registry
    {
        choices.extend(registry.routers.iter().map(|profile| {
            let mut choice = PickerChoice::available(
                PickerSourceContext::ConfiguredHosted(profile.clone()),
                profile.name.as_str(),
            );
            choice.secondary = profile
                .default_remote_cwd
                .as_ref()
                .map(|cwd| cwd.as_str().to_owned());
            if profile.default_remote_cwd.is_none() {
                choice.availability = Availability::Unavailable {
                    reason: "Configure a destination working directory before creating a session"
                        .to_owned(),
                };
            } else if !matches!(profile.transport, RouterConnectionTransport::Ssh(_)) {
                let reason = match profile.native_codex() {
                    None => "Interactive session route is unavailable",
                    Some(native)
                        if native.credential.is_some()
                            && !native.address.allows_credential_reference() =>
                    {
                        "Credential reference is unsupported by this native transport"
                    }
                    Some(_) => "Execution route needs verification; no session created",
                };
                choice.availability = Availability::Unavailable {
                    reason: reason.to_owned(),
                };
            }
            choice
        }));
    }
    choices
}

pub(crate) fn client_choices(model: &SessionsPickerModel) -> Vec<PickerChoice<NewSessionClient>> {
    let Some(draft) = model.new_session.as_ref() else {
        return Vec::new();
    };
    [
        NewSessionClient::CodexNative,
        NewSessionClient::ClaudeNativeForeground,
        NewSessionClient::CursorManagedAcp,
    ]
    .into_iter()
    .map(|client| {
        let selected = if client == NewSessionClient::ClaudeNativeForeground
            && draft.selected_source != PickerSourceContext::DefaultHosted
        {
            NewSessionClient::ClaudeNativeBackground
        } else {
            client
        };
        PickerChoice {
            key: selected,
            label: match client {
                NewSessionClient::CodexNative => "Codex",
                NewSessionClient::ClaudeNativeForeground
                | NewSessionClient::ClaudeNativeBackground => "Claude Code",
                NewSessionClient::CursorManagedAcp => "Cursor · managed ACP",
            }
            .into(),
            secondary: None,
            availability: client_availability(draft, selected),
        }
    })
    .collect()
}

pub(crate) fn run_choices(model: &SessionsPickerModel) -> Vec<PickerChoice<NewSessionClient>> {
    let Some(draft) = model.new_session.as_ref() else {
        return Vec::new();
    };
    if !matches!(
        draft.client,
        NewSessionClient::ClaudeNativeForeground | NewSessionClient::ClaudeNativeBackground
    ) || draft.selected_source != PickerSourceContext::DefaultHosted
    {
        return Vec::new();
    }
    [
        NewSessionClient::ClaudeNativeForeground,
        NewSessionClient::ClaudeNativeBackground,
    ]
    .into_iter()
    .map(|client| PickerChoice {
        key: client,
        label: if client == NewSessionClient::ClaudeNativeForeground {
            "Foreground"
        } else {
            "Background"
        }
        .into(),
        secondary: None,
        availability: client_availability(draft, client),
    })
    .collect()
}

pub(crate) fn client_availability(
    draft: &NewSessionDraft,
    client: NewSessionClient,
) -> Availability {
    use crate::session_picker::new_session::new_session_client_inventory::CursorCreationAvailability;
    use crate::session_picker::new_session::new_session_client_inventory::NewClientInventoryState;
    use agent_session_inventory::sessions::source_ssh_protocol::SourceNativeLaunchAvailability;
    let unavailable = |reason: String| Availability::Unavailable { reason };
    if client == NewSessionClient::CodexNative {
        return Availability::Available;
    }
    if draft.selected_source == PickerSourceContext::LocalCodex {
        return unavailable("Local mode supports Codex only".to_owned());
    }
    if client == NewSessionClient::ClaudeNativeForeground
        && draft.selected_source != PickerSourceContext::DefaultHosted
    {
        return unavailable("Foreground Claude is available on this machine only. Use background mode for this connection.".to_owned());
    }
    match &draft.client_inventory {
        NewClientInventoryState::Unobserved => {
            unavailable("Selected source client capabilities have not been checked".to_owned())
        }
        NewClientInventoryState::Checking => {
            unavailable("Checking selected source clients".to_owned())
        }
        NewClientInventoryState::Unavailable(reason) => unavailable(reason.clone()),
        NewClientInventoryState::Ready(capabilities) => match client {
            NewSessionClient::CodexNative => Availability::Available,
            NewSessionClient::ClaudeNativeForeground | NewSessionClient::ClaudeNativeBackground => {
                match &capabilities.native_claude {
                    SourceNativeLaunchAvailability::Ready { .. } => Availability::Available,
                    SourceNativeLaunchAvailability::Unavailable { message } => {
                        unavailable(message.clone())
                    }
                }
            }
            NewSessionClient::CursorManagedAcp if !draft.arguments.is_empty() => {
                unavailable("Cursor creation does not accept these native arguments".to_owned())
            }
            NewSessionClient::CursorManagedAcp => match &capabilities.cursor {
                CursorCreationAvailability::Ready(_) => Availability::Available,
                CursorCreationAvailability::Unavailable(reason) => unavailable(reason.clone()),
            },
        },
    }
}

impl SessionsPickerModel {
    pub(crate) fn open_new_form(&mut self) {
        if self.new_session.is_none()
            && !self.client_action.is_active()
            && self.source_action.pending.is_none()
            && self.provider_filter_draft.is_none()
        {
            self.details_dialog = None;
            self.next_client_generation = self.next_client_generation.saturating_add(1);
            self.new_session = Some(NewSessionDraft::capture(self));
        }
    }
    pub(crate) fn select_new_machine(&mut self, source: PickerSourceContext) {
        let directory = source_directory(&source, &self.request.current_dir);
        self.next_client_generation = self.next_client_generation.saturating_add(1);
        if let Some(draft) = self.new_session.as_mut() {
            draft.generation = self.next_client_generation;
            draft.client_inventory =
                crate::session_picker::new_session::new_session_client_inventory::NewClientInventoryState::Unobserved;
            draft.selected_source = source;
            draft.directory = directory;
        }
    }
    pub(crate) fn submit_new_form(&mut self) -> Option<SessionsPickerOutcome> {
        if self.source_action.pending.is_some() || self.client_action.is_active() || self.show_help
        {
            return None;
        }
        let draft = self.new_session.as_ref()?.clone();
        if draft.client != NewSessionClient::CodexNative {
            self.begin_new_client_action(&draft);
            return None;
        }
        match draft.selected_source {
            PickerSourceContext::DefaultHosted | PickerSourceContext::LocalCodex => {
                self.new_session = None;
                Some(SessionsPickerOutcome::StartNewSession)
            }
            PickerSourceContext::ConfiguredHosted(profile) => {
                if !machine_choices(self).iter().any(|choice| {
                    choice.key == PickerSourceContext::ConfiguredHosted(profile.clone())
                        && choice.availability == Availability::Available
                }) {
                    return None;
                }
                self.begin_configured_new();
                None
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod new_session_form_test;
