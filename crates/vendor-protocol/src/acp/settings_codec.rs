//! Observations from ACP v1 session settings replies and updates.

use agent_client_protocol_schema::v1::{
    SessionConfigId, SessionConfigKind, SessionConfigOption, SessionConfigOptionCategory,
    SessionConfigSelectOption, SessionConfigSelectOptions, SessionConfigValueId,
};

/// The settings roles recognized from ACP categories and provider option IDs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingCategory {
    Mode,
    Model,
    Effort,
    Fast,
}

/// A decoded ACP setting with its original ID and label and an optional recognized role.
#[derive(Debug, Eq, PartialEq)]
pub struct ObservedSetting<'option> {
    pub id: &'option SessionConfigId,
    pub name: &'option str,
    pub category: Option<SettingCategory>,
    pub value: ObservedSettingValue<'option>,
}

/// A provider's current selection or native Boolean value.
#[derive(Debug, Eq, PartialEq)]
pub enum ObservedSettingValue<'option> {
    Select {
        /// The selected ID, which may be absent from the offered choices.
        current: &'option SessionConfigValueId,
        /// Decoded choices in provider order, flattened across groups.
        ///
        /// `None` means the codec does not recognize the select offer representation.
        /// `Some` with an empty list means the decoded representation contains no choices.
        offered: Option<Vec<&'option SessionConfigSelectOption>>,
    },
    Boolean(bool),
}

/// Project already-decoded ACP v1 settings options.
///
/// `None` preserves absence in the decoded input. `Some` with an empty list means
/// projection produced zero options. A malformed field can produce the same
/// decoded state as an omitted field or an explicit empty list.
///
/// On typed session replies and updates, the pinned ACP schema can skip malformed
/// or unrecognized entries and default a malformed `configOptions` field to
/// absence or an empty list, depending on the message. A list whose entries are
/// all skipped is also empty. Skipped-entry warnings require the upstream
/// `tracing` feature.
///
/// The Router's set-config-option reply path rejects the entire array if any
/// item fails to decode before it calls this function.
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AdvertisedEffort {
    ThinkingToggle,
    Explicit,
    Categorized,
    Unclassified,
}

pub(super) fn classify_advertised_effort(
    category: Option<&SessionConfigOptionCategory>,
    id: &str,
) -> AdvertisedEffort {
    // Cursor advertises both the thinking toggle and reasoning effort under thought_level.
    match (id, category) {
        ("thinking", _) => AdvertisedEffort::ThinkingToggle,
        ("effort", _) => AdvertisedEffort::Explicit,
        (_, Some(SessionConfigOptionCategory::ThoughtLevel)) => AdvertisedEffort::Categorized,
        _ => AdvertisedEffort::Unclassified,
    }
}

fn setting_category(
    category: Option<&SessionConfigOptionCategory>,
    id: &str,
) -> Option<SettingCategory> {
    match classify_advertised_effort(category, id) {
        AdvertisedEffort::ThinkingToggle => None,
        AdvertisedEffort::Explicit | AdvertisedEffort::Categorized => Some(SettingCategory::Effort),
        AdvertisedEffort::Unclassified => match category {
            Some(SessionConfigOptionCategory::Mode) => Some(SettingCategory::Mode),
            Some(SessionConfigOptionCategory::Model) => Some(SettingCategory::Model),
            _ => match id {
                "mode" => Some(SettingCategory::Mode),
                "model" => Some(SettingCategory::Model),
                "thought_level" => Some(SettingCategory::Effort),
                // Cursor places Fast in its broad model_config category.
                "fast" => Some(SettingCategory::Fast),
                _ => None,
            },
        },
    }
}

#[cfg(test)]
#[path = "settings_codec_test.rs"]
mod tests;
