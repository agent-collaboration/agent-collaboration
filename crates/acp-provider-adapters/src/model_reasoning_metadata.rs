//! Pure decoders for provider model descriptions; never discovers or selects models.

use acp_client_runtime::{
    ModelReasoning, ModelReasoningEntry, ModelReasoningMetadata, ModelReasoningSelect,
    ModelReasoningUnknown, ProviderSettingChoice,
};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashSet;

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

#[derive(Deserialize)]
struct CursorModels {
    models: Vec<CursorModel>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CursorModel {
    value: String,
    name: String,
    config_options: Vec<CursorOption>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CursorOption {
    id: String,
    category: Option<String>,
    #[serde(rename = "type")]
    kind: String,
    current_value: Value,
    options: Option<Vec<CursorChoice>>,
}
#[derive(Deserialize)]
struct CursorChoice {
    value: String,
    name: String,
}

pub(crate) fn decode_cursor_models(value: &Value) -> ModelReasoningMetadata {
    let decoded = || -> Result<ModelReasoningMetadata, ModelReasoningUnknown> {
        let response: CursorModels =
            serde_json::from_value(value.clone()).map_err(|_| ModelReasoningUnknown::Malformed)?;
        let entries = response
            .models
            .into_iter()
            .map(|row| {
                if row.name.is_empty() || row.name.contains('\0') {
                    return Err(ModelReasoningUnknown::Malformed);
                }
                let reasoning =
                    cursor_reasoning(&row.config_options).unwrap_or_else(ModelReasoning::Unknown);
                ModelReasoningEntry::try_new(row.value, reasoning)
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

fn cursor_reasoning(options: &[CursorOption]) -> Result<ModelReasoning, ModelReasoningUnknown> {
    let candidates = options
        .iter()
        .filter(|option| {
            option.id != "thinking"
                && option.kind == "select"
                && (option.id == "effort" || option.category.as_deref() == Some("thought_level"))
        })
        .collect::<Vec<_>>();
    let preferred = candidates
        .iter()
        .copied()
        .filter(|option| option.id == "effort")
        .collect::<Vec<_>>();
    let eligible = if preferred.is_empty() {
        candidates.as_slice()
    } else {
        preferred.as_slice()
    };
    let option = match eligible {
        [] => return Err(ModelReasoningUnknown::Unavailable),
        [option] => *option,
        _ => return Err(ModelReasoningUnknown::Ambiguous),
    };
    if options.iter().filter(|other| other.id == option.id).count() != 1 {
        return Err(ModelReasoningUnknown::Ambiguous);
    }
    let default_value = option
        .current_value
        .as_str()
        .ok_or(ModelReasoningUnknown::Malformed)?
        .to_owned();
    let choices = option
        .options
        .as_ref()
        .ok_or(ModelReasoningUnknown::Malformed)?
        .iter()
        .map(|choice| ProviderSettingChoice {
            value: choice.value.clone(),
            label: choice.name.clone(),
        })
        .collect();
    ModelReasoningSelect::try_new(option.id.clone(), choices, default_value)
        .map(ModelReasoning::Select)
}

#[cfg(test)]
mod tests {
    use super::*;
    use acp_client_runtime::{ProviderBehavior, ProviderSettingKind};
    use serde_json::json;

    #[test]
    fn confirmation_reuse_is_qualified_by_provider_and_setting_scope() {
        for kind in [
            ProviderSettingKind::Mode,
            ProviderSettingKind::Model,
            ProviderSettingKind::Effort,
        ] {
            assert_eq!(
                crate::CursorAcpBehavior.can_reuse_setting_confirmation(kind),
                kind == ProviderSettingKind::Mode
            );
            assert!(
                crate::ClaudeAcpBehavior::new(crate::ClaudeSkillSource::Annotated)
                    .can_reuse_setting_confirmation(kind)
            );
            assert!(
                !crate::ClaudeAcpBehavior::new(crate::ClaudeSkillSource::Unavailable(
                    session_event_model::SkillSourceError::UnestablishedClassification
                ))
                .can_reuse_setting_confirmation(kind)
            );
            assert!(!crate::StandardAcpBehavior.can_reuse_setting_confirmation(kind));
        }
    }

    #[test]
    fn literal_cursor_rows_keep_unknown_models_and_their_own_vendor_defaults() {
        // Installed Cursor7096 list_available_models response, not live current settings.
        let decoded = decode_cursor_models(&json!({"models":[
            {"value":"model-a","name":"A","configOptions":[
                {"id":"reasoning","name":"Reasoning","category":"thought_level","type":"select",
                 "currentValue":"low","options":[{"value":"low","name":"Low"},{"value":"high","name":"High"}]}
            ]},
            {"value":"model-b","name":"B","configOptions":[]}
        ]}));
        let ModelReasoning::Select(control) = decoded.for_model("model-a") else {
            panic!("qualified select")
        };
        assert_eq!(control.config_id(), "reasoning");
        assert_eq!(control.default_value(), "low");
        assert_eq!(control.choices()[1].value, "high");
        assert_eq!(
            decoded.for_model("model-b"),
            ModelReasoning::Unknown(ModelReasoningUnknown::Unavailable)
        );
        assert_eq!(
            decoded.for_model("foreign"),
            ModelReasoning::Unknown(ModelReasoningUnknown::Unavailable)
        );
    }

    #[test]
    fn literal_claude_annotation_requires_offered_default_and_unique_model_identity() {
        let valid = json!({"version":1,"kind":"available","models":[
            {"modelId":"sonnet","kind":"select","configId":"effort","defaultValue":"default",
             "choices":[{"value":"default","label":"Default"},{"value":"high","label":"High"}]},
            {"modelId":"haiku","kind":"unknown","reason":"Unavailable"}
        ]});
        let decoded = decode_claude_models(&valid);
        let ModelReasoning::Select(control) = decoded.for_model("sonnet") else {
            panic!("qualified select")
        };
        assert_eq!(control.default_value(), "default");
        assert_eq!(
            decoded.for_model("haiku"),
            ModelReasoning::Unknown(ModelReasoningUnknown::Unavailable)
        );
        let mut invalid = valid.clone();
        invalid["models"][0]["defaultValue"] = json!("medium");
        assert_eq!(
            decode_claude_models(&invalid),
            ModelReasoningMetadata::Unknown(ModelReasoningUnknown::Malformed)
        );
        invalid = valid;
        invalid["models"][1]["modelId"] = json!("sonnet");
        assert_eq!(
            decode_claude_models(&invalid),
            ModelReasoningMetadata::Unknown(ModelReasoningUnknown::Ambiguous)
        );
    }
}
