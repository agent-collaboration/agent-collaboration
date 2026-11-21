//! Observations from ACP v1 session settings replies and updates.

use agent_client_protocol_schema::v1::{
    SessionConfigId, SessionConfigKind, SessionConfigOption, SessionConfigOptionCategory,
    SessionConfigSelectGroup, SessionConfigSelectOption, SessionConfigSelectOptions,
    SessionConfigValueId, SessionNotification, SessionUpdate,
};
use serde::Deserialize;
use serde_json::Value;

/// A session notification that cannot be applied by the ACP runtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionNotificationError {
    Malformed,
    InvalidSettingsArray,
    IncompleteSettingsOptions,
    InvalidSettingsMetadata,
    MalformedSettingsNotification,
}

/// Decode ACP v1 session/update parameters. Settings updates are full snapshots,
/// so reject them if the pinned decoder defaulted a field or skipped an option.
pub fn decode_session_notification(
    params: &Value,
) -> Result<SessionNotification, SessionNotificationError> {
    use SessionNotificationError::{
        IncompleteSettingsOptions, InvalidSettingsArray, InvalidSettingsMetadata, Malformed,
        MalformedSettingsNotification,
    };

    let is_settings_update = |update: &Value| {
        update.get("sessionUpdate").and_then(Value::as_str) == Some("config_option_update")
    };
    let Some(raw_settings) = params
        .get("update")
        .filter(|update| is_settings_update(update))
    else {
        // Serde also accepts positional structs. Reject this settings shape before
        // decoding so even an unrouted positional frame cannot emit skip warnings.
        if params.get(1).is_some_and(is_settings_update) {
            return Err(MalformedSettingsNotification);
        }
        let notification = SessionNotification::deserialize(params).map_err(|_| Malformed)?;
        return if matches!(&notification.update, SessionUpdate::ConfigOptionUpdate(_)) {
            Err(MalformedSettingsNotification)
        } else {
            Ok(notification)
        };
    };

    let raw_options = raw_settings
        .get("configOptions")
        .and_then(Value::as_array)
        .ok_or(InvalidSettingsArray)?;
    if !valid_optional_field(raw_settings, "_meta", Value::is_object) {
        return Err(InvalidSettingsMetadata);
    }
    // ACP's skip listener includes provider values in its warnings. This synchronous
    // settings decode uses a thread-local no-op dispatcher. The caller reports the
    // resulting rejection reason through its ordinary telemetry dispatcher.
    let notification = tracing::dispatcher::with_default(&tracing::Dispatch::none(), || {
        SessionNotification::deserialize(params)
    })
    .map_err(|_| MalformedSettingsNotification)?;
    let SessionUpdate::ConfigOptionUpdate(settings) = &notification.update else {
        return Err(MalformedSettingsNotification);
    };
    if !complete_list(
        raw_options,
        &settings.config_options,
        complete_config_option,
    ) {
        return Err(IncompleteSettingsOptions);
    }
    Ok(notification)
}

fn complete_config_option(raw: &Value, option: &SessionConfigOption) -> bool {
    if !raw.is_object() || !valid_optional_field(raw, "category", Value::is_string) {
        return false;
    }
    match &option.kind {
        SessionConfigKind::Boolean(_) => true,
        SessionConfigKind::Select(select) => {
            let Some(raw_options) = raw.get("options").and_then(Value::as_array) else {
                return false;
            };
            match &select.options {
                SessionConfigSelectOptions::Ungrouped(choices) => {
                    complete_list(raw_options, choices, |raw, _| complete_flat_choice(raw))
                }
                SessionConfigSelectOptions::Grouped(groups) => {
                    complete_list(raw_options, groups, complete_config_group)
                }
                _ => false,
            }
        }
        _ => false,
    }
}

fn complete_flat_choice(raw: &Value) -> bool {
    // ACP's untagged enum tries flat choices first. If a group also contains a
    // value, the flat decode can succeed while discarding its nested choices.
    let has_group_shape =
        raw.get("group").is_some_and(Value::is_string) && raw.get("options").is_some();
    raw.is_object() && !has_group_shape
}

fn complete_config_group(raw: &Value, group: &SessionConfigSelectGroup) -> bool {
    raw.get("options")
        .and_then(Value::as_array)
        .is_some_and(|choices| complete_list(choices, &group.options, |raw, _| raw.is_object()))
}

fn complete_list<Decoded>(
    raw: &[Value],
    decoded: &[Decoded],
    complete_entry: impl Fn(&Value, &Decoded) -> bool,
) -> bool {
    raw.len() == decoded.len()
        && raw
            .iter()
            .zip(decoded)
            .all(|(raw, decoded)| complete_entry(raw, decoded))
}

// Check optional fields that affect the catalog or update-level settings facts.
// ACP can default ancillary descriptions and option, group and choice metadata.
fn valid_optional_field(raw: &Value, field: &str, valid: fn(&Value) -> bool) -> bool {
    raw.get(field)
        .is_none_or(|value| value.is_null() || valid(value))
}

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
/// `tracing` feature. Use [`decode_session_notification`] for full snapshots
/// received as session updates.
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
