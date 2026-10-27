//! One invocation-local New draft; existing source preparation retains launch authority.
use super::{PickerSourceContext, SessionsPickerOutcome, picker_model::SessionsPickerModel};
use agent_session_inventory::sessions::{
    RouterRegistryRead, router_connection_registry::RouterConnectionTransport,
};
use codex_router_tui::{
    ChoiceAvailability, ChoiceOption, ChoiceTransition, DialogAction, DialogFrame,
    DialogKeyboardOwner, DialogMargins, DialogPresentation, FormField, SearchSelect,
    SearchSelectState,
};
use iocraft::prelude::*;
use std::{ffi::OsString, path::PathBuf};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum NewFormFocus {
    Client,
    Machine,
    Directory,
    Cancel,
    #[default]
    Start,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum NewSessionClient {
    #[default]
    CodexNative,
    ClaudeNativeBackground,
    CursorManagedAcp,
}

impl NewSessionClient {
    fn label(self) -> &'static str {
        match self {
            Self::CodexNative => "Codex",
            Self::ClaudeNativeBackground => "Claude Code · background",
            Self::CursorManagedAcp => "Cursor · managed ACP",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum NewSessionChoiceKey {
    Client(NewSessionClient),
    Machine(PickerSourceContext),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct NewSessionDraft {
    pub(super) generation: u64,
    pub(super) client_inventory: super::new_session_client_inventory::NewClientInventoryState,
    pub(super) client: NewSessionClient,
    pub(super) selected_source: PickerSourceContext,
    pub(super) directory: PathBuf,
    pub(super) arguments: Vec<OsString>,
    pub(super) choice: SearchSelectState<NewSessionChoiceKey>,
    pub(super) focus: NewFormFocus,
}

impl NewSessionDraft {
    fn capture(model: &SessionsPickerModel) -> Self {
        let source = match &model.machine_controls.filter {
            super::picker_machine_controls::PickerMachineFilter::Single { source } => {
                source.as_ref().clone()
            }
            _ => match model.request.machine_mode {
                super::PickerMachineSourceMode::HostedDefault => PickerSourceContext::DefaultHosted,
                super::PickerMachineSourceMode::LocalCodex => PickerSourceContext::LocalCodex,
            },
        };
        let directory = source_directory(&source, &model.request.current_dir);
        Self {
            generation: model.next_client_generation,
            client_inventory:
                super::new_session_client_inventory::NewClientInventoryState::Unobserved,
            client: NewSessionClient::CodexNative,
            selected_source: source,
            directory,
            arguments: model.request.native_arguments.clone(),
            choice: SearchSelectState::Closed,
            focus: NewFormFocus::Start,
        }
    }
    fn move_focus(&mut self, reverse: bool) {
        let fields = [
            NewFormFocus::Client,
            NewFormFocus::Machine,
            NewFormFocus::Directory,
            NewFormFocus::Cancel,
            NewFormFocus::Start,
        ];
        let index = fields
            .iter()
            .position(|field| *field == self.focus)
            .unwrap_or(0);
        self.focus = fields
            .get((index + if reverse { fields.len() - 1 } else { 1 }) % fields.len())
            .copied()
            .unwrap_or(NewFormFocus::Client);
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

pub(super) fn machine_choices(
    model: &SessionsPickerModel,
) -> Vec<ChoiceOption<PickerSourceContext>> {
    let source = match model.request.machine_mode {
        super::PickerMachineSourceMode::HostedDefault => PickerSourceContext::DefaultHosted,
        super::PickerMachineSourceMode::LocalCodex => PickerSourceContext::LocalCodex,
    };
    let mut choices = vec![ChoiceOption::available(
        source.clone(),
        source_label(&source),
    )];
    if model.request.machine_mode == super::PickerMachineSourceMode::HostedDefault
        && let RouterRegistryRead::Ready(registry) = &model.request.router_registry
    {
        choices.extend(registry.routers.iter().map(|profile| {
            let mut choice = ChoiceOption::available(
                PickerSourceContext::ConfiguredHosted(profile.clone()),
                profile.name.as_str(),
            );
            choice.secondary = profile
                .default_remote_cwd
                .as_ref()
                .map(|cwd| cwd.as_str().to_owned());
            if profile.default_remote_cwd.is_none() {
                choice.availability = ChoiceAvailability::Unavailable {
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
                choice.availability = ChoiceAvailability::Unavailable {
                    reason: reason.to_owned(),
                };
            }
            choice
        }));
    }
    choices
}

pub(super) fn active_new_choices(
    model: &SessionsPickerModel,
) -> Vec<ChoiceOption<NewSessionChoiceKey>> {
    if model
        .new_session
        .as_ref()
        .is_some_and(|draft| draft.focus == NewFormFocus::Client)
    {
        [
            NewSessionClient::CodexNative,
            NewSessionClient::ClaudeNativeBackground,
            NewSessionClient::CursorManagedAcp,
        ]
        .into_iter()
        .map(|client| {
            let mut choice =
                ChoiceOption::available(NewSessionChoiceKey::Client(client), client.label());
            if client != NewSessionClient::CodexNative {
                choice.availability = model.new_session.as_ref().map_or_else(
                    || ChoiceAvailability::Unavailable {
                        reason: "New session is closed".to_owned(),
                    },
                    |draft| client_availability(draft, client),
                );
            }
            choice
        })
        .collect()
    } else {
        machine_choices(model)
            .into_iter()
            .map(|choice| ChoiceOption {
                key: NewSessionChoiceKey::Machine(choice.key),
                label: choice.label,
                secondary: choice.secondary,
                availability: choice.availability,
            })
            .collect()
    }
}

pub(super) fn client_availability(
    draft: &NewSessionDraft,
    client: NewSessionClient,
) -> ChoiceAvailability {
    use super::new_session_client_inventory::{
        CursorCreationAvailability, NewClientInventoryState,
    };
    use agent_session_inventory::sessions::source_ssh_protocol::SourceNativeLaunchAvailability;
    let unavailable = |reason: String| ChoiceAvailability::Unavailable { reason };
    if client == NewSessionClient::CodexNative {
        return ChoiceAvailability::Available;
    }
    if draft.selected_source == PickerSourceContext::LocalCodex {
        return unavailable("Local mode supports Codex only".to_owned());
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
            NewSessionClient::CodexNative => ChoiceAvailability::Available,
            NewSessionClient::ClaudeNativeBackground => match &capabilities.native_claude {
                SourceNativeLaunchAvailability::Ready => ChoiceAvailability::Available,
                SourceNativeLaunchAvailability::Unavailable { message } => {
                    unavailable(message.clone())
                }
            },
            NewSessionClient::CursorManagedAcp if !draft.arguments.is_empty() => {
                unavailable("Cursor creation does not accept these native arguments".to_owned())
            }
            NewSessionClient::CursorManagedAcp => match &capabilities.cursor {
                CursorCreationAvailability::Ready(_) => ChoiceAvailability::Available,
                CursorCreationAvailability::Unavailable(reason) => unavailable(reason.clone()),
            },
        },
    }
}

impl SessionsPickerModel {
    pub(super) fn select_new_choice(&mut self, key: NewSessionChoiceKey) {
        match key {
            NewSessionChoiceKey::Machine(source) => self.select_new_machine(source),
            NewSessionChoiceKey::Client(client) => {
                if !active_new_choices(self).iter().any(|choice| {
                    choice.key == NewSessionChoiceKey::Client(client)
                        && choice.availability == ChoiceAvailability::Available
                }) {
                    return;
                }
                if let Some(draft) = self.new_session.as_mut() {
                    draft.client = client;
                    draft.choice = SearchSelectState::Closed;
                }
            }
        }
    }
    pub(super) fn open_new_form(&mut self) {
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
    pub(super) fn select_new_machine(&mut self, source: PickerSourceContext) {
        let directory = source_directory(&source, &self.request.current_dir);
        self.next_client_generation = self.next_client_generation.saturating_add(1);
        if let Some(draft) = self.new_session.as_mut() {
            draft.generation = self.next_client_generation;
            draft.client_inventory =
                super::new_session_client_inventory::NewClientInventoryState::Unobserved;
            draft.selected_source = source;
            draft.directory = directory;
            draft.choice = SearchSelectState::Closed;
        }
    }
    pub(super) fn submit_new_form(&mut self) -> Option<SessionsPickerOutcome> {
        if self.source_action.pending.is_some() || self.client_action.is_active() || self.show_help
        {
            return None;
        }
        let draft = self.new_session.as_ref()?.clone();
        if draft.choice.is_open() {
            return None;
        }
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
                        && choice.availability == ChoiceAvailability::Available
                }) {
                    return None;
                }
                self.begin_configured_new();
                None
            }
        }
    }
}

pub(super) fn handle_new_key(
    model: &mut SessionsPickerModel,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> Option<SessionsPickerOutcome> {
    let choices = active_new_choices(model);
    let draft = model.new_session.as_mut()?;
    if draft.choice.is_open() {
        let transition = draft.choice.handle_key(code, &choices);
        if let ChoiceTransition::Selected(key) = transition {
            model.select_new_choice(key);
        }
        return None;
    }
    match code {
        KeyCode::Esc => {
            model.new_session = None;
            model.source_action.error = None;
        }
        KeyCode::Tab | KeyCode::BackTab => {
            draft.move_focus(code == KeyCode::BackTab || modifiers.contains(KeyModifiers::SHIFT))
        }
        KeyCode::Enter if !modifiers.intersects(KeyModifiers::ALT | KeyModifiers::SUPER) => {
            match draft.focus {
                NewFormFocus::Client => {
                    draft.choice.open(NewSessionChoiceKey::Client(draft.client))
                }
                NewFormFocus::Machine => draft
                    .choice
                    .open(NewSessionChoiceKey::Machine(draft.selected_source.clone())),
                NewFormFocus::Directory => {}
                NewFormFocus::Cancel => {
                    model.new_session = None;
                }
                NewFormFocus::Start => return model.submit_new_form(),
            }
        }
        _ => {}
    }
    None
}

pub(super) fn render_new_form(
    model: &SessionsPickerModel,
    mut state: State<SessionsPickerModel>,
    mut outcome: State<Option<SessionsPickerOutcome>>,
    height: usize,
) -> AnyElement<'static> {
    let Some(draft) = model.new_session.as_ref() else {
        return element!(View).into_any();
    };
    let machine = source_label(&draft.selected_source);
    let choices = machine_choices(model);
    let selected_availability = choices
        .iter()
        .find(|choice| choice.key == draft.selected_source)
        .map(|choice| &choice.availability);
    let can_start = client_availability(draft, draft.client) == ChoiceAvailability::Available
        && matches!(selected_availability, Some(ChoiceAvailability::Available));
    let unavailable_reason = match selected_availability {
        Some(ChoiceAvailability::Unavailable { reason }) => Some(reason.clone()),
        None => Some("Selected machine is unavailable".to_owned()),
        Some(ChoiceAvailability::Available) => None,
    };
    let choice = draft.choice.clone();
    let expanded = choice.is_open();
    let width = u16::try_from(model.width).unwrap_or(u16::MAX);
    let height = u16::try_from(height).unwrap_or(u16::MAX);
    let ((_, frame_width), _) =
        DialogPresentation::default().resolve(width.saturating_sub(4), height.saturating_sub(2));
    // The longest field label, gap and searchable value fit in38 content cells.
    // Below that, use the shared stacked form instead of clipping either column.
    let stacked = frame_width.saturating_sub(4) < 38;
    let client_field = if choice.is_open() && draft.focus == NewFormFocus::Client {
        element!(SearchSelect<NewSessionChoiceKey>(state:choice.clone(),options:active_new_choices(model),on_select:move |key| {let mut state=state;state.write().select_new_choice(key);})).into_any()
    } else {
        element! {codex_router_tui::DialogButton(handler:move |_| {if let Some(draft)=state.write().new_session.as_mut() {draft.focus=NewFormFocus::Client;draft.choice.open(NewSessionChoiceKey::Client(draft.client));}}) {
            Text(content:format!("{} ▾",draft.client.label()),color:if draft.focus==NewFormFocus::Client {Color::Yellow}else {Color::White})
        }}.into_any()
    };
    let machine_field = if choice.is_open() && draft.focus == NewFormFocus::Machine {
        element!(SearchSelect<NewSessionChoiceKey>(state:choice,options:active_new_choices(model),on_select:move |key| {let mut state=state;state.write().select_new_choice(key);})).into_any()
    } else {
        element! {
            codex_router_tui::DialogButton(handler:move |_| {if let Some(draft)=state.write().new_session.as_mut() {draft.focus=NewFormFocus::Machine;draft.choice.open(NewSessionChoiceKey::Machine(draft.selected_source.clone()));}}) {
                Text(content:format!("{machine} ▾"),color:if draft.focus==NewFormFocus::Machine {Color::Yellow}else {Color::White})
            }
        }.into_any()
    };
    let enabled = if expanded {
        draft
            .choice
            .available_focused_key(&active_new_choices(model))
            .is_some()
    } else {
        can_start
    };
    let actions=vec![
        element!(DialogAction(label:"Cancel",keyboard_owner:DialogKeyboardOwner::Caller,has_focus:draft.focus==NewFormFocus::Cancel,handler:move |_|state.write().new_session=None)).into_any(),
        element!(DialogAction(label:if expanded {"Select"}else {"Start"},enabled,keyboard_owner:DialogKeyboardOwner::Caller,has_focus:!expanded&&draft.focus==NewFormFocus::Start,handler:move |_| {
            if expanded {
                let selected={let current=state.read();current.new_session.as_ref().and_then(|draft|draft.choice.available_focused_key(&active_new_choices(&current)))};
                if let Some(key)=selected {state.write().select_new_choice(key);}
            }else {let selected=state.write().submit_new_form();if selected.is_some() {outcome.set(selected);}}
        })).into_any(),
    ];
    let arguments = model.request.new_session_args_display.clone();
    element! {
        DialogFrame(width,height,title:"New session".to_owned(),scroll_keys:if draft.choice.is_open() {codex_router_tui::DialogScrollKeys::Disabled}else {codex_router_tui::DialogScrollKeys::Navigation},keyboard_owner:DialogKeyboardOwner::Caller,
            margins:DialogMargins {top:1,right:2,bottom:1,left:2},help_action:Some(super::picker_help_view::help_action(state)),actions) {
            FormField(label:"Client".to_owned(),label_width:10u16,stacked,has_focus:draft.focus==NewFormFocus::Client) {#(if expanded&&draft.focus==NewFormFocus::Client {client_field}else {element!(codex_router_tui::SelectableRow(selected:false) {#(client_field)}).into_any()})}
            FormField(label:"Machine".to_owned(),label_width:10u16,stacked,has_focus:draft.focus==NewFormFocus::Machine) {#(if expanded&&draft.focus==NewFormFocus::Machine {machine_field}else {element!(codex_router_tui::SelectableRow(selected:false) {#(machine_field)}).into_any()})}
            FormField(label:"Directory".to_owned(),label_width:10u16,stacked,has_focus:draft.focus==NewFormFocus::Directory) {Text(content:if draft.directory.as_os_str().is_empty() {"Not configured".to_owned()}else {draft.directory.display().to_string()},color:Color::White,wrap:TextWrap::Wrap)}
            #((!arguments.is_empty()).then(||element! {FormField(label:"Arguments".to_owned(),label_width:10u16,stacked) {Text(content:arguments,color:Color::Grey,wrap:TextWrap::Wrap)}}))
            #(unavailable_reason.map(|reason|element!(Text(content:reason,color:Color::Grey))))
            #(super::source_action_view::source_error(model))
        }
    }.into_any()
}

#[cfg(test)]
#[path = "new_session_form_tests.rs"]
mod tests;
