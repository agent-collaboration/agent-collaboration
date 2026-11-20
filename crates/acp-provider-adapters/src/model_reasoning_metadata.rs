//! Pure decoders for provider model descriptions; never discovers or selects models.

use acp_client_runtime::{
    ModelReasoning, ModelReasoningEntry, ModelReasoningMetadata, ModelReasoningSelect,
    ModelReasoningUnknown, ProviderSettingChoice,
};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashSet;
use vendor_protocol::acp::cursor_model_discovery::{
    CursorModelReasoning, decode_cursor_model_list,
};

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Annotation {
    Unknown { reason: ModelReasoningUnknown },
    Available { models: Vec<AnnotatedModel> },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VersionedAnnotation {
    version: u64,
    #[serde(flatten)]
    annotation: Annotation,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum AnnotatedModel {
    #[serde(rename_all = "camelCase")]
    Unknown {
        model_id: String,
        reason: ModelReasoningUnknown,
    },
    #[serde(rename_all = "camelCase")]
    Select {
        model_id: String,
        config_id: String,
        default_value: String,
        choices: Vec<AnnotatedChoice>,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AnnotatedChoice {
    value: String,
    label: String,
}

pub(crate) fn decode_claude_models(value: &Value) -> ModelReasoningMetadata {
    let decoded = || -> Result<ModelReasoningMetadata, ModelReasoningUnknown> {
        let annotation: VersionedAnnotation =
            serde_json::from_value(value.clone()).map_err(|_| ModelReasoningUnknown::Malformed)?;
        if annotation.version != 1 {
            return Err(ModelReasoningUnknown::Unrecognized);
        }
        match annotation.annotation {
            Annotation::Unknown { reason } => Ok(ModelReasoningMetadata::Unknown(reason)),
            Annotation::Available { models } => {
                let entries = models
                    .into_iter()
                    .map(|row| {
                        let (model_id, reasoning) = match row {
                            AnnotatedModel::Unknown { model_id, reason } => {
                                (model_id, ModelReasoning::Unknown(reason))
                            }
                            AnnotatedModel::Select {
                                model_id,
                                config_id,
                                default_value,
                                choices,
                            } => {
                                let choices = choices
                                    .into_iter()
                                    .map(|choice| ProviderSettingChoice {
                                        value: choice.value,
                                        label: choice.label,
                                    })
                                    .collect();
                                (
                                    model_id,
                                    ModelReasoning::Select(ModelReasoningSelect::try_new(
                                        config_id,
                                        choices,
                                        default_value,
                                    )?),
                                )
                            }
                        };
                        ModelReasoningEntry::try_new(model_id, reasoning)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                unique_models(entries)
            }
        }
    };
    decoded().unwrap_or_else(ModelReasoningMetadata::Unknown)
}

pub(crate) fn decode_cursor_models(value: &Value) -> ModelReasoningMetadata {
    let decoded = || -> Result<ModelReasoningMetadata, ModelReasoningUnknown> {
        let descriptors =
            decode_cursor_model_list(value).map_err(|_| ModelReasoningUnknown::Malformed)?;
        let entries = descriptors
            .into_iter()
            .map(|descriptor| {
                let reasoning = match descriptor.reasoning {
                    CursorModelReasoning::Select {
                        config_id,
                        default_value,
                        choices,
                    } => ModelReasoningSelect::try_new(
                        config_id.to_string(),
                        choices
                            .into_iter()
                            .map(|choice| ProviderSettingChoice {
                                value: choice.value.to_string(),
                                label: choice.name,
                            })
                            .collect(),
                        default_value.to_string(),
                    )
                    .map_or_else(ModelReasoning::Unknown, ModelReasoning::Select),
                    CursorModelReasoning::Unavailable => {
                        ModelReasoning::Unknown(ModelReasoningUnknown::Unavailable)
                    }
                    CursorModelReasoning::Ambiguous => {
                        ModelReasoning::Unknown(ModelReasoningUnknown::Ambiguous)
                    }
                    CursorModelReasoning::Malformed => {
                        ModelReasoning::Unknown(ModelReasoningUnknown::Malformed)
                    }
                };
                ModelReasoningEntry::try_new(descriptor.model_id, reasoning)
            })
            .collect::<Result<Vec<_>, _>>()?;
        unique_models(entries)
    };
    decoded().unwrap_or_else(ModelReasoningMetadata::Unknown)
}

fn unique_models(
    entries: Vec<ModelReasoningEntry>,
) -> Result<ModelReasoningMetadata, ModelReasoningUnknown> {
    let mut ids = HashSet::new();
    if entries.iter().any(|row| !ids.insert(row.model_id())) {
        return Err(ModelReasoningUnknown::Ambiguous);
    }
    Ok(ModelReasoningMetadata::Available {
        per_model_descriptors: entries,
    })
}

#[cfg(test)]
#[path = "model_reasoning_metadata_test.rs"]
mod tests;
