//! Typed choices bridge existing picker drafts to scoped crate combo boxes.
use super::{
    ActionDef, ActionId, ComboBoxState, HelpControl, PickerDialogKind, PickerMachineFilter,
    PickerSurfaceContext, Placement, SurfaceId, SurfaceResult, SurfaceStackHandle, action, target,
};
use crate::session_picker::{
    PickerSourceContext,
    new_session_form::NewSessionClient,
    picker_fork_confirmation::ForkDestinationKey,
    picker_machine_controls::MachineChoiceKey,
    provider_filter_model::{ProviderFilterFocus, SessionFilterValue},
};
use agent_collaboration_tui::{
    ChoiceAvailability, ChoiceOption,
    actions::Availability,
    controls::{
        combo::{ComboBoxBinding, ComboBoxControl},
        list::{OptionKey, SelectOption},
    },
    surfaces::{
        focus_model::{GroupId, TargetSpec},
        surface_handle::SurfaceContext,
    },
};
use iocraft::prelude::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ChoiceField {
    NewClient,
    NewMachine,
    NewRun,
    Provider,
    Folder,
    ModelProvider,
    Source,
    Runtime,
    Sort,
    BrowseMachine,
    ForkDestination,
}
impl ChoiceField {
    pub fn index(self) -> usize {
        match self {
            Self::NewClient => 0,
            Self::NewMachine => 1,
            Self::NewRun => 2,
            Self::Provider => 3,
            Self::Folder => 4,
            Self::ModelProvider => 5,
            Self::Source => 6,
            Self::Runtime => 7,
            Self::Sort => 8,
            Self::BrowseMachine => 9,
            Self::ForkDestination => 10,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::NewClient => "new-client",
            Self::NewMachine => "new-machine",
            Self::NewRun => "new-run",
            Self::Provider => "filter-provider",
            Self::Folder => "filter-folder",
            Self::ModelProvider => "filter-model-provider",
            Self::Source => "filter-source",
            Self::Runtime => "filter-runtime",
            Self::Sort => "filter-sort",
            Self::BrowseMachine => "browse-machine",
            Self::ForkDestination => "fork-destination",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::NewClient => "Client",
            Self::NewMachine | Self::BrowseMachine => "Machine",
            Self::NewRun => "Run",
            Self::Provider => "Client provider",
            Self::Folder => "Folder",
            Self::ModelProvider => "Model provider",
            Self::Source => "Source",
            Self::Runtime => "Runtime",
            Self::Sort => "Sort",
            Self::ForkDestination => "Destination",
        }
    }
    fn filter(self) -> Option<ProviderFilterFocus> {
        match self {
            Self::Provider => Some(ProviderFilterFocus::Provider),
            Self::Folder => Some(ProviderFilterFocus::Folder),
            Self::ModelProvider => Some(ProviderFilterFocus::ModelProvider),
            Self::Source => Some(ProviderFilterFocus::Source),
            Self::Runtime => Some(ProviderFilterFocus::Runtime),
            Self::Sort => Some(ProviderFilterFocus::Sort),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PickerChoiceValue {
    Client(NewSessionClient),
    Machine(PickerSourceContext),
    Run(NewSessionClient),
    Filter(SessionFilterValue),
    Browse(MachineChoiceKey),
    Fork(ForkDestinationKey),
}
fn source_key(source: &PickerSourceContext) -> String {
    match source {
        PickerSourceContext::DefaultHosted => "default".into(),
        PickerSourceContext::LocalCodex => "local".into(),
        PickerSourceContext::ConfiguredHosted(profile) => {
            format!("configured:{}", profile.name.as_str())
        }
    }
}
fn option_key(value: &PickerChoiceValue) -> SurfaceResult<OptionKey> {
    let text = match value {
        PickerChoiceValue::Client(client) => format!("client:{client:?}"),
        PickerChoiceValue::Machine(source) => format!("machine:{}", source_key(source)),
        PickerChoiceValue::Run(client) => format!("run:{client:?}"),
        PickerChoiceValue::Filter(value) => format!("filter:{value:?}"),
        PickerChoiceValue::Browse(MachineChoiceKey::Default) => "browse:default".into(),
        PickerChoiceValue::Browse(MachineChoiceKey::All) => "browse:all".into(),
        PickerChoiceValue::Browse(MachineChoiceKey::Configured(source)) => {
            format!("browse:{}", source_key(source))
        }
        PickerChoiceValue::Fork(ForkDestinationKey::Source) => "fork:source".into(),
        PickerChoiceValue::Fork(ForkDestinationKey::Machine(source)) => {
            format!("fork:{}", source_key(source))
        }
    };
    Ok(OptionKey::new(text)?)
}

fn values(
    context: &PickerSurfaceContext,
    field: ChoiceField,
) -> Vec<ChoiceOption<PickerChoiceValue>> {
    let model = context.model.read();
    let wrap = |choices: Vec<ChoiceOption<PickerChoiceValue>>| choices;
    if let Some(filter) = field.filter() {
        return crate::session_picker::provider_filter_model::filter_choices(&model, filter)
            .into_iter()
            .map(|option| ChoiceOption {
                key: PickerChoiceValue::Filter(option.key),
                label: option.label,
                secondary: option.secondary,
                availability: option.availability,
            })
            .collect();
    }
    match field {
        ChoiceField::NewClient => crate::session_picker::new_session_form::client_choices(&model)
            .into_iter()
            .map(|option| ChoiceOption {
                key: PickerChoiceValue::Client(option.key),
                label: option.label,
                secondary: option.secondary,
                availability: option.availability,
            })
            .collect(),
        ChoiceField::NewMachine => crate::session_picker::new_session_form::machine_choices(&model)
            .into_iter()
            .map(|option| ChoiceOption {
                key: PickerChoiceValue::Machine(option.key),
                label: option.label,
                secondary: option.secondary,
                availability: option.availability,
            })
            .collect(),
        ChoiceField::NewRun => crate::session_picker::new_session_form::run_choices(&model)
            .into_iter()
            .map(|option| ChoiceOption {
                key: PickerChoiceValue::Run(option.key),
                label: option.label,
                secondary: option.secondary,
                availability: option.availability,
            })
            .collect(),
        ChoiceField::BrowseMachine => model
            .machine_controls
            .choice_options(&model.request.router_registry, model.request.machine_mode)
            .into_iter()
            .map(|option| ChoiceOption {
                key: PickerChoiceValue::Browse(option.key),
                label: option.label,
                secondary: option.secondary,
                availability: option.availability,
            })
            .collect(),
        ChoiceField::ForkDestination => model
            .fork_confirmation
            .as_ref()
            .map(|confirmation| {
                confirmation
                    .choice_options()
                    .into_iter()
                    .map(|option| ChoiceOption {
                        key: PickerChoiceValue::Fork(option.key),
                        label: option.label,
                        secondary: option.secondary,
                        availability: option.availability,
                    })
                    .collect()
            })
            .unwrap_or_default(),
        _ => wrap(Vec::new()),
    }
}

fn selected(context: &PickerSurfaceContext, field: ChoiceField) -> Option<PickerChoiceValue> {
    let model = context.model.read();
    if let Some(filter) = field.filter() {
        return model
            .provider_filter_draft
            .as_ref()?
            .field_value(filter)
            .map(PickerChoiceValue::Filter);
    }
    match field {
        ChoiceField::NewClient => model.new_session.as_ref().map(|draft| {
            PickerChoiceValue::Client(
                if matches!(
                    draft.client,
                    NewSessionClient::ClaudeNativeForeground
                        | NewSessionClient::ClaudeNativeBackground
                ) && draft.selected_source == PickerSourceContext::DefaultHosted
                {
                    NewSessionClient::ClaudeNativeForeground
                } else {
                    draft.client
                },
            )
        }),
        ChoiceField::NewMachine => model
            .new_session
            .as_ref()
            .map(|draft| PickerChoiceValue::Machine(draft.selected_source.clone())),
        ChoiceField::NewRun => model
            .new_session
            .as_ref()
            .map(|draft| PickerChoiceValue::Run(draft.client)),
        ChoiceField::BrowseMachine => Some(PickerChoiceValue::Browse(
            match &model.machine_controls.filter {
                PickerMachineFilter::Default => MachineChoiceKey::Default,
                PickerMachineFilter::All => MachineChoiceKey::All,
                PickerMachineFilter::Single { source } => {
                    MachineChoiceKey::Configured(source.as_ref().clone())
                }
            },
        )),
        ChoiceField::ForkDestination => model.fork_confirmation.as_ref().and_then(|confirmation| {
            confirmation
                .destinations
                .get(confirmation.focused_destination)
                .map(|destination| PickerChoiceValue::Fork(destination.key.clone()))
        }),
        _ => None,
    }
}

fn options(context: &PickerSurfaceContext, field: ChoiceField) -> SurfaceResult<Vec<SelectOption>> {
    values(context, field)
        .into_iter()
        .map(|option| {
            Ok(SelectOption {
                key: option_key(&option.key)?,
                label: option.label,
                secondary: option.secondary,
                availability: match option.availability {
                    ChoiceAvailability::Available => Availability::Available,
                    ChoiceAvailability::Unavailable { reason } => {
                        Availability::Unavailable { reason }
                    }
                },
            })
        })
        .collect()
}
pub(super) fn fields(context: &PickerSurfaceContext, kind: PickerDialogKind) -> Vec<ChoiceField> {
    match kind {
        PickerDialogKind::New => {
            let mut fields = vec![ChoiceField::NewClient, ChoiceField::NewMachine];
            if context
                .model
                .read()
                .new_session
                .as_ref()
                .is_some_and(|draft| {
                    matches!(
                        draft.client,
                        NewSessionClient::ClaudeNativeForeground
                            | NewSessionClient::ClaudeNativeBackground
                    ) && draft.selected_source == PickerSourceContext::DefaultHosted
                })
            {
                fields.push(ChoiceField::NewRun);
            }
            fields
        }
        PickerDialogKind::Filters => vec![
            ChoiceField::Provider,
            ChoiceField::Folder,
            ChoiceField::ModelProvider,
            ChoiceField::Source,
            ChoiceField::Runtime,
            ChoiceField::Sort,
        ],
        PickerDialogKind::Machine => vec![ChoiceField::BrowseMachine],
        PickerDialogKind::Fork => vec![ChoiceField::ForkDestination],
        _ => Vec::new(),
    }
}
pub(super) fn initialize_choices(
    context: &PickerSurfaceContext,
    kind: PickerDialogKind,
) -> SurfaceResult<()> {
    for field in fields(context, kind) {
        let mut state = combo_state(context, field)?;
        state.set(ComboBoxState::new(
            options(context, field)?,
            selected(context, field)
                .as_ref()
                .map(option_key)
                .transpose()?,
        )?);
    }
    Ok(())
}
pub(super) fn synchronize_choices(
    context: &PickerSurfaceContext,
    stack: SurfaceStackHandle,
) -> SurfaceResult<()> {
    let Some(active) = context.active_dialog.read().clone() else {
        return Ok(());
    };
    for field in fields(context, active.kind) {
        let next = options(context, field)?;
        let mut value = combo_state(context, field)?;
        let selected = selected(context, field)
            .as_ref()
            .map(option_key)
            .transpose()?;
        if value.read().value() != selected.as_ref() {
            value.set(ComboBoxState::new(next, selected)?);
        } else if value.read().options() != next.as_slice() {
            control(context, stack, SurfaceId::Overlay(active.id), field)?.replace_options(next)?;
        }
    }
    Ok(())
}

pub(super) fn declare_combo(
    context: &PickerSurfaceContext,
    stack: SurfaceStackHandle,
    help: &HelpControl,
    field: ChoiceField,
    actions: &mut Vec<ActionDef>,
    targets: &mut Vec<TargetSpec>,
) -> SurfaceResult<()> {
    let open_context = context.clone();
    let help = help.clone();
    let mut declared = action(
        field.name(),
        field.label(),
        &[],
        Placement::Hidden,
        move |()| {
            match control(&open_context, stack, stack.snapshot().top_id(), field)
                .and_then(|control| Ok(control.open()?))
            {
                Ok(_) => {}
                Err(error) => open_context.report(error),
            }
            open_context.synchronize_dialog(stack, &help);
        },
    )?;
    if field == ChoiceField::BrowseMachine {
        let model = context.model;
        declared.availability = super::AvailabilityFn::new(move || {
            if matches!(
                model.read().machine_controls.stage,
                crate::session_picker::picker_machine_controls::PickerMachineStage::Loading { .. }
            ) {
                Availability::Unavailable {
                    reason: "Loading selected machine".into(),
                }
            } else {
                Availability::Available
            }
        });
    }
    targets.push(
        agent_collaboration_tui::controls::fields::button_target(target(field.name())?, &declared)
            .in_group(GroupId::new("dialog-controls")?),
    );
    actions.push(declared);
    Ok(())
}

fn control(
    context: &PickerSurfaceContext,
    stack: SurfaceStackHandle,
    parent: SurfaceId,
    field: ChoiceField,
) -> SurfaceResult<ComboBoxControl> {
    let parent_scroll = context
        .active_dialog
        .read()
        .as_ref()
        .filter(|dialog| SurfaceId::Overlay(dialog.id) == parent)
        .map(|dialog| dialog.scroll.clone())
        .ok_or("missing scroll for the active picker dialog")?;
    let effect = context.clone();
    Ok(ComboBoxControl::new(ComboBoxBinding {
        parent: SurfaceContext {
            stack,
            surface: parent,
        },
        field: target(field.name())?,
        parent_scroll: Some(parent_scroll),
        value: combo_state(context, field)?,
        on_pick: Handler::from(move |key| {
            let choice = values(&effect, field)
                .into_iter()
                .find(|option| {
                    option_key(&option.key).ok().as_ref() == Some(&key)
                        && option.availability == ChoiceAvailability::Available
                })
                .map(|option| option.key);
            let Some(choice) = choice else {
                return;
            };
            choose(&effect, choice);
            if let Some(help) = effect.help.read().clone() {
                effect.synchronize_dialog(stack, &help);
            }
        }),
    }))
}

fn choose(context: &PickerSurfaceContext, choice: PickerChoiceValue) {
    let mut model = context.model;
    match choice {
        PickerChoiceValue::Client(client) | PickerChoiceValue::Run(client) => {
            if let Some(draft) = model.write().new_session.as_mut() {
                draft.client = client;
            }
        }
        PickerChoiceValue::Machine(source) => model.write().select_new_machine(source),
        PickerChoiceValue::Filter(value) => {
            if let Some(draft) = model.write().provider_filter_draft.as_mut() {
                draft.select_value(value);
            }
        }
        PickerChoiceValue::Fork(key) => {
            if let Some(confirmation) = model.write().fork_confirmation.as_mut() {
                confirmation.select_destination(key);
            }
        }
        PickerChoiceValue::Browse(key) => {
            super::super::picker_machine_selection::select_machine_choice(
                super::super::picker_machine_selection::MachineSelectionContext {
                    model,
                    can_load_source: context.can_load_source,
                    reload_port: context.reload_port.clone(),
                    reload_generation: context.reload_generation,
                    conversation_cache: context.conversation_cache,
                },
                key,
            )
        }
    }
}

pub(super) fn declare_choices(
    context: &PickerSurfaceContext,
    stack: SurfaceStackHandle,
    help: &HelpControl,
    kind: PickerDialogKind,
    actions: &mut Vec<ActionDef>,
    targets: &mut Vec<TargetSpec>,
) -> SurfaceResult<ActionId> {
    for field in fields(context, kind) {
        declare_combo(context, stack, help, field, actions, targets)?;
    }
    let effect = context.clone();
    let help = help.clone();
    let (name, title) = if kind == PickerDialogKind::Filters {
        ("filters-apply", "Apply")
    } else if kind == PickerDialogKind::Fork {
        ("fork-confirm", "Fork")
    } else {
        return Ok(ActionId::new("dialog-close")?);
    };
    let mut primary = action(
        name,
        title,
        &["Ctrl+Enter"],
        Placement::Toolbar,
        move |()| {
            if !effect.owns_dialog_input(stack, kind) {
                return;
            }
            if kind == PickerDialogKind::Filters {
                effect.change_query(|model| model.apply_provider_filters());
            } else {
                crate::session_picker::picker_fork_view::confirm_fork(effect.model, effect.outcome);
            }
            effect.synchronize_dialog(stack, &help);
        },
    )?;
    if kind == PickerDialogKind::Fork {
        let model = context.model;
        primary.availability = super::AvailabilityFn::new(move || {
            model
                .read()
                .fork_confirmation
                .as_ref()
                .and_then(|confirmation| {
                    confirmation
                        .destinations
                        .get(confirmation.focused_destination)
                })
                .and_then(|destination| destination.availability.explanation())
                .map_or(Availability::Available, |reason| {
                    Availability::Unavailable {
                        reason: reason.into(),
                    }
                })
        });
    }
    let id = primary.id.clone();
    if kind == PickerDialogKind::Filters {
        actions.insert(0, primary);
    } else {
        actions.push(primary);
    }
    Ok(id)
}

pub(super) fn combo_state(
    context: &PickerSurfaceContext,
    field: ChoiceField,
) -> SurfaceResult<State<ComboBoxState>> {
    context
        .combos
        .get(field.index())
        .copied()
        .ok_or_else(|| "missing declared combo state".into())
}
