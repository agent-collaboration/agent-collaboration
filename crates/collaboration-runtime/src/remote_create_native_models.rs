//! Native model advertisements are read per preflight, never guessed or cached here.
use crate::NativeControlBackend;
use codex_native_integration::{NativeConnectionError, NativeOperation, NativeProtocolConnection};
use collaboration_protocol::{CodexGeneration, ProviderRequestedSettings};
use serde::Deserialize;
use serde_json::json;
use std::{collections::BTreeSet, time::Duration};

#[derive(Debug)]
pub(super) enum NativeCreateSettingsError {
    InvalidInput,
    ModelNotAdvertised,
    EffortNotAdvertised,
    Unavailable,
    InvalidAdvertisement,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelPage {
    data: Vec<ModelAdvertisement>,
    next_cursor: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelAdvertisement {
    model: String,
    supported_reasoning_efforts: Vec<EffortAdvertisement>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EffortAdvertisement {
    reasoning_effort: String,
}

pub(super) async fn validate_native_settings(
    backend: &NativeControlBackend,
    settings: &ProviderRequestedSettings,
) -> Result<CodexGeneration, NativeCreateSettingsError> {
    let valid_choice = |value: &str| !value.is_empty() && !value.chars().any(char::is_whitespace);
    let model = settings
        .model
        .as_deref()
        .filter(|value| valid_choice(value))
        .ok_or(NativeCreateSettingsError::InvalidInput)?;
    let effort = settings
        .effort
        .as_deref()
        .filter(|value| valid_choice(value))
        .ok_or(NativeCreateSettingsError::InvalidInput)?;
    if settings.mode.is_some() {
        return Err(NativeCreateSettingsError::InvalidInput);
    }
    let admission = backend
        .gate
        .acquire()
        .map_err(|_| NativeCreateSettingsError::Unavailable)?;
    let schemas = admission
        .schemas()
        .ok_or(NativeCreateSettingsError::Unavailable)?;
    if !schemas.supports_operation(NativeOperation::ListModels) {
        // Only an advertised pre-creation choice is a membership constraint.
        return Ok(admission.generation().clone());
    }
    let retired = admission.retirement();
    let read = async {
        let mut connection = NativeProtocolConnection::connect(admission.backend_path())
            .await
            .map_err(|_| NativeCreateSettingsError::Unavailable)?;
        let mut cursor = None::<String>;
        let mut seen_cursors = BTreeSet::new();
        let mut model_seen = false;
        for _ in 0..128 {
            let response = connection
                .request_validated(
                    &schemas,
                    NativeOperation::ListModels,
                    json!({"cursor":cursor,"limit":100,"includeHidden":true}),
                )
                .await
                .map_err(|error| match error {
                    NativeConnectionError::InvalidInput | NativeConnectionError::Protocol => {
                        NativeCreateSettingsError::InvalidAdvertisement
                    }
                    _ => NativeCreateSettingsError::Unavailable,
                })?;
            let page: ModelPage = serde_json::from_value(response)
                .map_err(|_| NativeCreateSettingsError::InvalidAdvertisement)?;
            for advertisement in page.data {
                if !valid_choice(&advertisement.model)
                    || advertisement
                        .supported_reasoning_efforts
                        .iter()
                        .any(|choice| !valid_choice(&choice.reasoning_effort))
                {
                    return Err(NativeCreateSettingsError::InvalidAdvertisement);
                }
                if advertisement.model == model {
                    model_seen = true;
                    if advertisement
                        .supported_reasoning_efforts
                        .iter()
                        .any(|choice| choice.reasoning_effort == effort)
                    {
                        return Ok(());
                    }
                }
            }
            let Some(next) = page.next_cursor else {
                return Err(if model_seen {
                    NativeCreateSettingsError::EffortNotAdvertised
                } else {
                    NativeCreateSettingsError::ModelNotAdvertised
                });
            };
            if !seen_cursors.insert(next.clone()) {
                return Err(NativeCreateSettingsError::InvalidAdvertisement);
            }
            cursor = Some(next);
        }
        Err(NativeCreateSettingsError::InvalidAdvertisement)
    };
    let result = tokio::select! {
        biased;
        _ = retired.cancelled() => Err(NativeCreateSettingsError::Unavailable),
        result = tokio::time::timeout(Duration::from_secs(30), read) => result.unwrap_or(Err(NativeCreateSettingsError::Unavailable)),
    };
    result.map(|()| admission.generation().clone())
}
