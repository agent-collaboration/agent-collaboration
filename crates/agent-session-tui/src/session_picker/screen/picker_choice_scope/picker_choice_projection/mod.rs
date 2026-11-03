//! The ComboBox boundary projects typed picker identities and rejects unknown picks.
use crate::session_picker::screen::picker_choice::PickerChoice;
use crate::session_picker::screen::picker_choice_scope::ForkDestinationKey;
use crate::session_picker::screen::picker_choice_scope::MachineChoiceKey;
use crate::session_picker::screen::picker_choice_scope::NewSessionClient;
use crate::session_picker::screen::picker_choice_scope::PickerSourceContext;
use crate::session_picker::screen::picker_choice_scope::SessionFilterValue;
use crate::session_picker::screen::picker_choice_scope::SurfaceResult;
use agent_collaboration_tui::actions::Availability;
use agent_collaboration_tui::controls::list::OptionKey;
use agent_collaboration_tui::controls::list::SelectOption;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PickerChoiceValue {
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
pub(crate) fn option_key(value: &PickerChoiceValue) -> SurfaceResult<OptionKey> {
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

pub(crate) struct ProjectedPickerChoice {
    pub value: PickerChoiceValue,
    pub option: SelectOption,
}

pub(crate) fn project_choices(
    choices: Vec<PickerChoice<PickerChoiceValue>>,
) -> SurfaceResult<Vec<ProjectedPickerChoice>> {
    choices
        .into_iter()
        .map(|choice| {
            let key = option_key(&choice.key)?;
            Ok(ProjectedPickerChoice {
                value: choice.key,
                option: SelectOption {
                    key,
                    label: choice.label,
                    secondary: choice.secondary,
                    availability: choice.availability,
                },
            })
        })
        .collect()
}

pub(crate) fn resolve_choice(
    choices: &[ProjectedPickerChoice],
    key: &OptionKey,
) -> SurfaceResult<PickerChoiceValue> {
    let choice = choices
        .iter()
        .find(|choice| choice.option.key == *key)
        .ok_or_else(|| format!("Unknown picker choice key: {}", key.as_str()))?;
    match &choice.option.availability {
        Availability::Available => Ok(choice.value.clone()),
        Availability::Unavailable { reason } => {
            Err(format!("Picker choice is unavailable: {reason}").into())
        }
    }
}

#[cfg(test)]
pub(crate) mod picker_choice_projection_test;
