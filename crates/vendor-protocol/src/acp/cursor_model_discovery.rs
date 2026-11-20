//! Decode the connection-scoped `cursor/list_available_models` response.

use agent_client_protocol_schema::v1::{
    SessionConfigId, SessionConfigOptionCategory, SessionConfigSelectOption, SessionConfigValueId,
};
use serde::{Deserialize, Deserializer, de::value::StringDeserializer};
use serde_json::Value;

use super::settings_codec::{AdvertisedEffort, classify_advertised_effort};

#[derive(Deserialize)]
struct CursorModelList {
    models: Vec<CursorModel>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CursorModel {
    value: String,
    name: String,
    config_options: Vec<CursorConfigOption>,
}

// The discovery reply uses flat choices, as reflected in the Cursor7096 fixture.
// Keep every option ID for duplicate detection, including unrecognized kinds.
// The option label is unused here, and a missing offer list or a non-string
// current value only invalidates reasoning when that option is selected.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CursorConfigOption {
    id: SessionConfigId,
    #[serde(default, deserialize_with = "decode_category")]
    category: Option<SessionConfigOptionCategory>,
    #[serde(rename = "type")]
    kind: CursorOptionKind,
    current_value: Value,
    #[serde(default, deserialize_with = "decode_discovery_choices")]
    options: Option<Vec<SessionConfigSelectOption>>,
}

fn decode_discovery_choices<'de, D>(
    deserializer: D,
) -> Result<Option<Vec<SessionConfigSelectOption>>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<Vec<Value>>::deserialize(deserializer)?
        .map(|choices| {
            choices
                .into_iter()
                .map(|choice| {
                    // Discovery's positional form has two fields. The ACP type
                    // also accepts description and metadata in extra positions.
                    if let Some(fields) = choice.as_array()
                        && fields.len() != 2
                    {
                        return Err(serde::de::Error::invalid_length(
                            fields.len(),
                            &"two fields in a positional Cursor discovery choice",
                        ));
                    }
                    SessionConfigSelectOption::deserialize(choice).map_err(serde::de::Error::custom)
                })
                .collect()
        })
        .transpose()
}

#[derive(Deserialize)]
#[serde(from = "String")]
enum CursorOptionKind {
    Select,
    Other,
}

impl From<String> for CursorOptionKind {
    fn from(kind: String) -> Self {
        if kind == "select" {
            Self::Select
        } else {
            Self::Other
        }
    }
}

fn decode_category<'de, D>(deserializer: D) -> Result<Option<SessionConfigOptionCategory>, D::Error>
where
    D: Deserializer<'de>,
{
    // Preserve discovery's strict string field while sharing ACP's category vocabulary.
    Option::<String>::deserialize(deserializer)?
        .map(|name| SessionConfigOptionCategory::deserialize(StringDeserializer::new(name)))
        .transpose()
}

/// A vendor model ID and the reasoning advertised for that model.
pub struct CursorModelDescriptor {
    pub model_id: String,
    pub reasoning: CursorModelReasoning,
}

/// The reasoning description from a structurally valid model row.
pub enum CursorModelReasoning {
    Select {
        config_id: SessionConfigId,
        default_value: SessionConfigValueId,
        choices: Vec<SessionConfigSelectOption>,
    },
    Unavailable,
    Ambiguous,
    Malformed,
}

/// Decode the flat discovery reply as a whole before interpreting model rows.
/// Invalid rows or choice shapes fail the reply, including grouped offers.
pub fn decode_cursor_model_list(
    value: &Value,
) -> Result<Vec<CursorModelDescriptor>, serde_json::Error> {
    let response = CursorModelList::deserialize(value)?;
    response
        .models
        .into_iter()
        .map(|model| {
            if model.name.is_empty() || model.name.contains('\0') {
                return Err(serde::de::Error::custom("invalid Cursor model name"));
            }
            Ok(CursorModelDescriptor {
                model_id: model.value,
                reasoning: describe_reasoning(&model.config_options),
            })
        })
        .collect()
}

fn describe_reasoning(options: &[CursorConfigOption]) -> CursorModelReasoning {
    let (explicit, categorized): (Vec<_>, Vec<_>) = options
        .iter()
        .filter_map(|option| {
            if !matches!(option.kind, CursorOptionKind::Select) {
                return None;
            }
            match classify_advertised_effort(option.category.as_ref(), option.id.0.as_ref()) {
                role @ (AdvertisedEffort::Explicit | AdvertisedEffort::Categorized) => {
                    Some((role, option))
                }
                AdvertisedEffort::ThinkingToggle | AdvertisedEffort::Unclassified => None,
            }
        })
        .partition(|(role, _)| *role == AdvertisedEffort::Explicit);
    let eligible = if explicit.is_empty() {
        &categorized
    } else {
        &explicit
    };
    let option = match eligible.as_slice() {
        [] => return CursorModelReasoning::Unavailable,
        [(_, option)] => *option,
        _ => return CursorModelReasoning::Ambiguous,
    };
    if options
        .iter()
        .filter(|candidate| candidate.id == option.id)
        .count()
        != 1
    {
        return CursorModelReasoning::Ambiguous;
    }
    let (Ok(default_value), Some(choices)) = (
        SessionConfigValueId::deserialize(&option.current_value),
        &option.options,
    ) else {
        return CursorModelReasoning::Malformed;
    };
    CursorModelReasoning::Select {
        config_id: option.id.clone(),
        default_value,
        choices: choices.clone(),
    }
}
