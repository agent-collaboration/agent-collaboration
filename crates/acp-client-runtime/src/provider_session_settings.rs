//! Typed provider Session settings at the ACP client boundary.
use crate::provider_setting_choice::ProviderSettingChoice;
use provider_session_machines::ProviderSettingKind;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RequestedProviderSettings {
    pub mode: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub fast: FastRequest,
}

impl RequestedProviderSettings {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.mode.is_none()
            && self.model.is_none()
            && self.effort.is_none()
            && self.fast == FastRequest::Leave
    }
}

/// What a request asks of the agent's fast option, and where it came from
/// (D1 spec R7, R10). The Router never turns fast on.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FastRequest {
    /// Don't touch fast: `--fast auto`, and every setter that isn't about fast.
    #[default]
    Leave,
    /// The create default: turn fast off when the agent offers it; a no-op
    /// when it doesn't.
    OffIfOffered,
    /// An explicit fast off: with no fast option it's an invalid setting.
    Off,
}

/// What the agent last reported for its fast option.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FastObserved {
    Off,
    On,
    /// The agent offers no fast option.
    NoOption,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EffectiveProviderSettings {
    pub mode: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub fast: Option<FastObserved>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppliedProviderSetting {
    pub kind: ProviderSettingKind,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderSettingRequest {
    pub kind: ProviderSettingKind,
    pub value: String,
    pub intent: session_event_model::SettingApplicationIntent,
    pub expected_model: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FailedProviderSetting {
    pub kind: ProviderSettingKind,
    pub value: String,
    pub reason: session_event_model::ProviderSettingFailureReason,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotAppliedProviderSetting {
    pub kind: ProviderSettingKind,
    pub value: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvalidSettingSessionDisposition {
    Closed,
    RemainsCreated,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderConfigValue {
    Select(String),
    Boolean(bool),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderConfigOption {
    pub id: String,
    pub name: String,
    pub category: Option<ProviderSettingKind>,
    pub current_value: ProviderConfigValue,
    pub choices: Vec<ProviderSettingChoice>,
}

/// Last options advertised by a provider Session. The Host supervisor may keep
/// this snapshot after the Session unloads for model/list and settings inspect.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProviderSettingsCatalog {
    pub current_mode: Option<String>,
    pub modes: Vec<ProviderSettingChoice>,
    pub config_options: Vec<ProviderConfigOption>,
    pub model_reasoning: crate::ModelReasoningMetadata,
}

impl ProviderSettingsCatalog {
    /// Snapshot the values currently reported by the agent for the ordered
    /// Session event stream. Choice catalogs remain client-owned.
    #[must_use]
    pub(crate) fn to_session_settings(&self) -> session_event_model::SessionSettings {
        use session_event_model::{ConfigValue, ConfigValueState, SessionSettings};

        let effective = self.effective_settings();
        SessionSettings {
            mode: effective.mode,
            model: effective.model,
            effort: effective.effort,
            config: self
                .config_options
                .iter()
                .map(|option| ConfigValue {
                    id: option.id.clone(),
                    value: match &option.current_value {
                        ProviderConfigValue::Select(value) => {
                            ConfigValueState::Select(value.clone())
                        }
                        ProviderConfigValue::Boolean(value) => ConfigValueState::Boolean(*value),
                    },
                })
                .collect(),
        }
    }

    #[must_use]
    pub fn advertised_values(&self, kind: ProviderSettingKind) -> Vec<String> {
        if kind == ProviderSettingKind::Mode
            && !self
                .config_options
                .iter()
                .any(|option| option.category == Some(kind))
        {
            return self
                .modes
                .iter()
                .map(|choice| choice.value.clone())
                .collect();
        }
        self.config_option(kind)
            .map(|option| {
                option
                    .choices
                    .iter()
                    .map(|choice| choice.value.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    #[must_use]
    pub fn config_option(&self, kind: ProviderSettingKind) -> Option<&ProviderConfigOption> {
        if kind == ProviderSettingKind::Effort {
            let eligible = |option: &&ProviderConfigOption| {
                option.category == Some(kind)
                    && option.id != "thinking"
                    && matches!(option.current_value, ProviderConfigValue::Select(_))
            };
            let has_unique_config_id = |option: &ProviderConfigOption| {
                self.config_options
                    .iter()
                    .filter(|candidate| candidate.id == option.id)
                    .count()
                    == 1
            };
            let mut explicit = self
                .config_options
                .iter()
                .filter(eligible)
                .filter(|option| option.id == "effort");
            if let Some(option) = explicit.next() {
                return (explicit.next().is_none() && has_unique_config_id(option))
                    .then_some(option);
            }
            let mut fallback = self.config_options.iter().filter(eligible);
            let option = fallback.next()?;
            return (fallback.next().is_none() && has_unique_config_id(option)).then_some(option);
        }
        self.config_options
            .iter()
            .find(|option| option.category == Some(kind))
    }

    #[must_use]
    pub fn effective_settings(&self) -> EffectiveProviderSettings {
        let selected = |kind| {
            self.config_option(kind)
                .and_then(|option| match &option.current_value {
                    ProviderConfigValue::Select(value) => Some(value.clone()),
                    ProviderConfigValue::Boolean(_) => None,
                })
        };
        EffectiveProviderSettings {
            mode: selected(ProviderSettingKind::Mode).or_else(|| self.current_mode.clone()),
            model: selected(ProviderSettingKind::Model),
            effort: selected(ProviderSettingKind::Effort),
            fast: Some(self.fast_observed()),
        }
    }

    /// Whether this catalog offers a fast option at all.
    #[must_use]
    pub fn offers_fast(&self) -> bool {
        self.config_option(ProviderSettingKind::Fast).is_some()
    }

    /// The fast option's value as the agent reported it. Only an explicit
    /// "false" or false reads off, so fast is never reported off by guess.
    #[must_use]
    pub fn fast_observed(&self) -> FastObserved {
        match self.config_option(ProviderSettingKind::Fast) {
            None => FastObserved::NoOption,
            Some(option) => match &option.current_value {
                ProviderConfigValue::Select(value) if value == "false" => FastObserved::Off,
                ProviderConfigValue::Boolean(false) => FastObserved::Off,
                ProviderConfigValue::Select(_) | ProviderConfigValue::Boolean(true) => {
                    FastObserved::On
                }
            },
        }
    }
}

#[cfg(test)]
#[path = "provider_session_settings_test.rs"]
mod tests;
