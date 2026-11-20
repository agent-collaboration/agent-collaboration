//! Observations from ACP v1 session settings replies and updates.

use agent_client_protocol_schema::v1::{
    SessionConfigId, SessionConfigKind, SessionConfigOption, SessionConfigOptionCategory,
    SessionConfigSelectOption, SessionConfigSelectOptions, SessionConfigValueId,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingCategory {
    Mode,
    Model,
    Effort,
}

#[derive(Debug, Eq, PartialEq)]
pub struct ObservedSetting<'option> {
    pub id: &'option SessionConfigId,
    pub name: &'option str,
    pub category: Option<SettingCategory>,
    pub value: ObservedSettingValue<'option>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum ObservedSettingValue<'option> {
    Select {
        current: &'option SessionConfigValueId,
        offered: Option<Vec<&'option SessionConfigSelectOption>>,
    },
    Boolean(bool),
}

/// Preserve the difference between an omitted option set and a reported empty set.
///
/// ACP's decoder rejects unrecognized option tags before this function sees them.
#[must_use]
pub fn observe_config_options(
    options: Option<&[SessionConfigOption]>,
) -> Option<Vec<ObservedSetting<'_>>> {
    options.map(|options| options.iter().filter_map(observe_config_option).collect())
}

fn observe_config_option(option: &SessionConfigOption) -> Option<ObservedSetting<'_>> {
    let value = match &option.kind {
        SessionConfigKind::Select(select) => ObservedSettingValue::Select {
            current: &select.current_value,
            offered: match &select.options {
                SessionConfigSelectOptions::Ungrouped(options) => Some(options.iter().collect()),
                SessionConfigSelectOptions::Grouped(groups) => {
                    Some(groups.iter().flat_map(|group| &group.options).collect())
                }
                // Handle a future upstream shape without claiming it offered no choices.
                _ => None,
            },
        },
        SessionConfigKind::Boolean(boolean) => ObservedSettingValue::Boolean(boolean.current_value),
        _ => return None,
    };
    Some(ObservedSetting {
        id: &option.id,
        name: &option.name,
        category: setting_category(option.category.as_ref(), option.id.0.as_ref()),
        value,
    })
}

fn setting_category(
    category: Option<&SessionConfigOptionCategory>,
    id: &str,
) -> Option<SettingCategory> {
    // Cursor advertises both the thinking toggle and reasoning effort under thought_level.
    match id {
        "thinking" => return None,
        "effort" => return Some(SettingCategory::Effort),
        _ => {}
    }
    match category {
        Some(SessionConfigOptionCategory::Mode) => Some(SettingCategory::Mode),
        Some(SessionConfigOptionCategory::Model) => Some(SettingCategory::Model),
        Some(SessionConfigOptionCategory::ThoughtLevel) => Some(SettingCategory::Effort),
        _ => match id {
            "mode" => Some(SettingCategory::Mode),
            "model" => Some(SettingCategory::Model),
            "thought_level" => Some(SettingCategory::Effort),
            _ => None,
        },
    }
}

#[cfg(test)]
#[path = "settings_codec_test.rs"]
mod tests;
