//! Codex app-server model-list projection over the supervisor-owned live catalog.
use serde_json::{Value, json};
use std::{future::Future, pin::Pin};

pub trait SessionModelCatalog: Send + Sync {
    fn read_models(
        &self,
        session: message_board::SessionRef,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<Vec<ProviderModelEntry>, SessionModelCatalogError>>
                + Send
                + '_,
        >,
    >;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SessionModelCatalogError {
    #[error("session model catalogue not found")]
    SessionNotFound,
    #[error("session model catalogue unavailable")]
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderModelReasoning {
    ProviderSettings,
    Select {
        choices: Vec<ProviderReasoningChoice>,
        default_value: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderReasoningChoice {
    pub value: String,
    pub label: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderModelEntry {
    id: String,
    display_name: String,
    description: String,
    reasoning: ProviderModelReasoning,
}

#[derive(Debug, thiserror::Error)]
#[error("provider model entry requires a nonempty id and display name")]
pub struct InvalidProviderModelEntry;

impl ProviderModelEntry {
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn try_new(
        id: String,
        display_name: String,
        description: String,
    ) -> Result<Self, InvalidProviderModelEntry> {
        if id.is_empty()
            || display_name.is_empty()
            || id.contains('\0')
            || display_name.contains('\0')
        {
            return Err(InvalidProviderModelEntry);
        }
        Ok(Self {
            id,
            display_name,
            description,
            reasoning: ProviderModelReasoning::ProviderSettings,
        })
    }

    pub fn with_reasoning(
        mut self,
        choices: Vec<ProviderReasoningChoice>,
        default_value: String,
    ) -> Result<Self, InvalidProviderModelEntry> {
        let mut seen = std::collections::HashSet::new();
        if choices.is_empty()
            || !choices.iter().any(|choice| choice.value == default_value)
            || choices.iter().any(|choice| {
                choice.value.is_empty()
                    || choice.label.is_empty()
                    || choice.value.contains('\0')
                    || choice.label.contains('\0')
                    || !seen.insert(&choice.value)
            })
        {
            return Err(InvalidProviderModelEntry);
        }
        // The reserved action cannot also name an actual provider value.
        if !choices.iter().any(|choice| choice.value == "provider") {
            self.reasoning = ProviderModelReasoning::Select {
                choices,
                default_value,
            };
        }
        Ok(self)
    }

    #[must_use]
    pub fn reasoning(&self) -> &ProviderModelReasoning {
        &self.reasoning
    }

    fn provider_default() -> Self {
        Self {
            id: "provider-default".into(),
            display_name: "Use provider settings".into(),
            description: "Use provider settings".into(),
            reasoning: ProviderModelReasoning::ProviderSettings,
        }
    }

    fn as_app_server_model(&self, is_default: bool) -> Value {
        let (choices, default_value) = match &self.reasoning {
            ProviderModelReasoning::ProviderSettings => (
                vec![json!({"reasoningEffort":"provider","description":"Use provider settings"})],
                "provider",
            ),
            ProviderModelReasoning::Select {
                choices,
                default_value,
            } => (
                choices
                    .iter()
                    .map(
                        |choice| json!({"reasoningEffort":choice.value,"description":choice.label}),
                    )
                    .collect(),
                default_value.as_str(),
            ),
        };
        json!({
            "id":self.id,"model":self.id,
            "upgrade":null,"upgradeInfo":null,"availabilityNux":null,
            "displayName":self.display_name,"description":self.description,
            "hidden":false,"supportedReasoningEfforts":choices,
            "defaultReasoningEffort":default_value,"multiAgentVersion":null,
            "isDefault":is_default
        })
    }
}

pub fn render_model_list(catalog: &[ProviderModelEntry]) -> Value {
    let models = if catalog.is_empty() {
        vec![ProviderModelEntry::provider_default()]
    } else {
        catalog.to_vec()
    };
    json!({
        "data":models.iter().enumerate()
            .map(|(index, model)| model.as_app_server_model(index == 0))
            .collect::<Vec<_>>(),
        "nextCursor":null
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_delegates_to_provider_without_fabricated_effort() {
        // Program Design Native: bootstrap is an action, never a measured medium.
        let response = render_model_list(&[]);
        assert_eq!(response["data"][0]["id"], "provider-default");
        assert_eq!(response["data"][0]["defaultReasoningEffort"], "provider");
        assert_eq!(
            response["data"][0]["supportedReasoningEfforts"],
            json!([
                {"reasoningEffort":"provider", "description":"Use provider settings"}
            ])
        );
        assert_eq!(response["data"][0]["displayName"], "Use provider settings");
    }

    #[test]
    #[expect(
        clippy::panic_in_result_fn,
        reason = "Contract asserts; invalid fixture constructors propagate."
    )]
    fn all_models_keep_independent_defaults_and_reserved_collisions_delegate()
    -> Result<(), InvalidProviderModelEntry> {
        let first = ProviderModelEntry::try_new("a".into(), "A".into(), String::new())?
            .with_reasoning(
                vec![ProviderReasoningChoice {
                    value: "low".into(),
                    label: "Low".into(),
                }],
                "low".into(),
            )?;
        let unknown = ProviderModelEntry::try_new("b".into(), "B".into(), String::new())?;
        let collision = ProviderModelEntry::try_new("c".into(), "C".into(), String::new())?
            .with_reasoning(
                vec![ProviderReasoningChoice {
                    value: "provider".into(),
                    label: "Vendor level".into(),
                }],
                "provider".into(),
            )?;
        let response = render_model_list(&[first, unknown, collision]);
        assert_eq!(response["data"].as_array().map(Vec::len), Some(3));
        assert_eq!(response["data"][0]["defaultReasoningEffort"], "low");
        assert_eq!(response["data"][1]["defaultReasoningEffort"], "provider");
        assert_eq!(
            response["data"][2]["supportedReasoningEfforts"][0]["description"],
            "Use provider settings"
        );
        Ok(())
    }
}
