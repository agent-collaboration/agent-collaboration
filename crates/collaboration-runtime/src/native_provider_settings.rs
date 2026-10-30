//! Native setting precedence and supported provider writes, without mode aliases.
use super::{
    Identity, ProviderModelEntry, SessionCommandPort, ThreadMethodError, Value, command_error,
};
use collaboration_contracts::SetSessionSettingCommand;
use message_board::SessionRef;

pub(super) enum NativeSettingsSource {
    TurnStart,
    SettingsUpdate,
}

pub(super) struct NativeProviderSettings {
    model: Option<String>,
    effort: Option<String>,
}

pub(super) fn parse_native_settings(
    params: &Value,
    catalog: &[ProviderModelEntry],
    source: NativeSettingsSource,
) -> Result<NativeProviderSettings, ThreadMethodError> {
    if matches!(source, NativeSettingsSource::SettingsUpdate) {
        let unsupported = [
            "approvalPolicy",
            "approvalsReviewer",
            "permissions",
            "sandboxPolicy",
            "cwd",
            "disabledPluginIds",
            "personality",
            "summary",
            "multiAgentMode",
        ];
        if unsupported
            .iter()
            .any(|key| params.get(key).is_some_and(|value| !value.is_null()))
            || params.get("serviceTier").is_some()
        {
            return Err(ThreadMethodError::Unsupported);
        }
    }
    let top_model = optional_string(params.get("model"))?;
    let top_effort = optional_string(params.get("effort"))?;
    let (model, effort) = match params.get("collaborationMode") {
        None | Some(Value::Null) => (top_model, top_effort),
        Some(mode) => {
            match mode.get("mode").and_then(Value::as_str) {
                Some("plan") => return Err(ThreadMethodError::Unsupported),
                Some("default") => {}
                _ => return Err(ThreadMethodError::InvalidParams),
            }
            let settings = mode
                .get("settings")
                .and_then(Value::as_object)
                .ok_or(ThreadMethodError::InvalidParams)?;
            match settings.get("developer_instructions") {
                None | Some(Value::Null) => {}
                Some(Value::String(_)) => return Err(ThreadMethodError::Unsupported),
                Some(_) => return Err(ThreadMethodError::InvalidParams),
            }
            let model = settings
                .get("model")
                .and_then(Value::as_str)
                .ok_or(ThreadMethodError::InvalidParams)?;
            // A complete native mask replaces effort, unlike a sparse top-level
            // update. No provider-neutral model-default reset is qualified here.
            let effort = optional_string(settings.get("reasoning_effort"))?
                .ok_or(ThreadMethodError::Unsupported)?;
            (Some(model), Some(effort))
        }
    };
    if effort.is_some_and(str::is_empty) {
        return Err(ThreadMethodError::InvalidParams);
    }
    Ok(NativeProviderSettings {
        model: provider_model_override(model, catalog)?,
        effort: effort.map(str::to_owned),
    })
}

fn optional_string(value: Option<&Value>) -> Result<Option<&str>, ThreadMethodError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        Some(_) => Err(ThreadMethodError::InvalidParams),
    }
}

pub(super) async fn apply_native_settings(
    settings: NativeProviderSettings,
    commands: &dyn SessionCommandPort,
    target: &SessionRef,
    actor: &Identity,
) -> Result<(), ThreadMethodError> {
    // Each existing setting command validates against the provider's fresh catalogue.
    // A later refusal retains earlier confirmed effects; this adapter never rolls back.
    for (setting_id, value) in [("model", settings.model), ("effort", settings.effort)] {
        if let Some(value) = value {
            commands
                .set_setting(SetSessionSettingCommand {
                    target: target.clone(),
                    setting_id: setting_id.into(),
                    value,
                    actor: actor.clone(),
                })
                .await
                .map_err(command_error)?;
        }
    }
    Ok(())
}

pub(super) fn provider_model_override(
    requested: Option<&str>,
    catalog: &[ProviderModelEntry],
) -> Result<Option<String>, ThreadMethodError> {
    let Some(requested) = requested else {
        return Ok(None);
    };
    if requested == "provider-default" {
        return Ok(None);
    }
    if catalog.iter().any(|model| model.id() == requested) {
        return Ok(Some(requested.to_owned()));
    }
    Err(ThreadMethodError::InvalidParams)
}
