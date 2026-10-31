//! Validated per-model controls, separate from current effective settings.

use crate::provider_setting_choice::ProviderSettingChoice;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub enum ModelReasoningUnknown {
    UnsupportedSource,
    #[default]
    Unavailable,
    Malformed,
    Ambiguous,
    Unrecognized,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModelMetadataSource {
    SessionMetadata,
    ConnectionRequest { method: &'static str },
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModelReasoning {
    Unknown(ModelReasoningUnknown),
    Select(ModelReasoningSelect),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelReasoningSelect {
    config_id: String,
    choices: Vec<ProviderSettingChoice>,
    default_value: String,
}

impl ModelReasoningSelect {
    pub fn try_new(
        config_id: String,
        choices: Vec<ProviderSettingChoice>,
        default_value: String,
    ) -> Result<Self, ModelReasoningUnknown> {
        let valid = |value: &str| !value.is_empty() && !value.contains('\0');
        let mut values = HashSet::new();
        if !valid(&config_id)
            || !valid(&default_value)
            || choices.is_empty()
            || choices
                .iter()
                .any(|choice| !valid(&choice.value) || !valid(&choice.label))
            || !choices.iter().any(|choice| choice.value == default_value)
        {
            return Err(ModelReasoningUnknown::Malformed);
        }
        if choices.iter().any(|choice| !values.insert(&choice.value)) {
            return Err(ModelReasoningUnknown::Ambiguous);
        }
        Ok(Self {
            config_id,
            choices,
            default_value,
        })
    }

    #[must_use]
    pub fn config_id(&self) -> &str {
        &self.config_id
    }
    #[must_use]
    pub fn choices(&self) -> &[ProviderSettingChoice] {
        &self.choices
    }
    #[must_use]
    pub fn default_value(&self) -> &str {
        &self.default_value
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelReasoningEntry {
    model_id: String,
    reasoning: ModelReasoning,
}

impl ModelReasoningEntry {
    pub fn try_new(
        model_id: String,
        reasoning: ModelReasoning,
    ) -> Result<Self, ModelReasoningUnknown> {
        if model_id.is_empty() || model_id.contains('\0') {
            return Err(ModelReasoningUnknown::Malformed);
        }
        Ok(Self {
            model_id,
            reasoning,
        })
    }
    #[must_use]
    pub fn model_id(&self) -> &str {
        &self.model_id
    }
    #[must_use]
    pub fn reasoning(&self) -> &ModelReasoning {
        &self.reasoning
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModelReasoningMetadata {
    Unknown(ModelReasoningUnknown),
    Available {
        per_model_descriptors: Vec<ModelReasoningEntry>,
    },
}

impl Default for ModelReasoningMetadata {
    fn default() -> Self {
        Self::Unknown(ModelReasoningUnknown::Unavailable)
    }
}

impl ModelReasoningMetadata {
    #[must_use]
    pub fn for_model(&self, model_id: &str) -> ModelReasoning {
        match self {
            Self::Unknown(reason) => ModelReasoning::Unknown(*reason),
            Self::Available {
                per_model_descriptors,
            } => {
                let mut matches = per_model_descriptors
                    .iter()
                    .filter(|entry| entry.model_id() == model_id);
                match (matches.next(), matches.next()) {
                    (Some(entry), None) => entry.reasoning().clone(),
                    (None, _) => ModelReasoning::Unknown(ModelReasoningUnknown::Unavailable),
                    _ => ModelReasoning::Unknown(ModelReasoningUnknown::Ambiguous),
                }
            }
        }
    }
}
