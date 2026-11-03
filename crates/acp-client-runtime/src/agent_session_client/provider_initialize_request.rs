//! ACP v1 initialization and the injected provider's required authentication.

use super::provider_client_contract::{
    ExternalProviderRuntimeError, sanitized_acp_error, sanitized_initialization_error,
};

use agent_client_protocol::schema::v1::{AuthMethod, AuthenticateRequest, InitializeResponse};
use agent_client_protocol::{Agent, ConnectionTo, Error, UntypedMessage};
use serde_json::{Value, json};

use crate::ProviderInitializationAuthentication;

pub(super) async fn initialize_provider_connection(
    connection: &ConnectionTo<Agent>,
    model_picker: crate::ProviderModelPicker,
    authentication: ProviderInitializationAuthentication,
) -> Result<InitializeResponse, ExternalProviderRuntimeError> {
    let mut capabilities = serde_json::Map::from_iter([
        ("auth".to_owned(), json!({"terminal": false})),
        ("elicitation".to_owned(), json!({"form": {}})),
    ]);
    if model_picker == crate::ProviderModelPicker::CursorParameterized {
        capabilities.insert(
            "_meta".to_owned(),
            json!({"parameterizedModelPicker": true}),
        );
    }
    let request = UntypedMessage::new(
        "initialize",
        json!({
            "protocolVersion": 1,
            "clientCapabilities": capabilities,
            "clientInfo": {"name": "codex-router", "version": env!("CARGO_PKG_VERSION")},
        }),
    )
    .map_err(initialization_error)?;
    let response = connection
        .send_request(request)
        .block_task()
        .await
        .map_err(initialization_error)?;
    // Retain the raw selected offer: the SDK skips malformed rows and its Agent
    // fallback can otherwise erase an unknown discriminator or duplicate.
    let authenticate = if response.get("protocolVersion").and_then(Value::as_u64) == Some(1) {
        required_authentication_request(&response, authentication).map_err(initialization_error)?
    } else {
        None
    };
    let initialized = serde_json::from_value(response)
        .map_err(|_| initialization_error(Error::internal_error()))?;
    if let Some(request) = authenticate {
        connection
            .send_request(request)
            .block_task()
            .await
            .map_err(|error| {
                ExternalProviderRuntimeError::Initialize(sanitized_acp_error(
                    &error,
                    "authenticate",
                    "initialize",
                ))
            })?;
    }
    Ok(initialized)
}

fn initialization_error(error: Error) -> ExternalProviderRuntimeError {
    ExternalProviderRuntimeError::Initialize(sanitized_initialization_error(&error))
}

fn required_authentication_request(
    response: &Value,
    policy: ProviderInitializationAuthentication,
) -> Result<Option<AuthenticateRequest>, Error> {
    let ProviderInitializationAuthentication::RequiredAgentMethod { method_id } = policy else {
        return Ok(None);
    };
    let offers = response
        .get("authMethods")
        .and_then(Value::as_array)
        .ok_or_else(Error::invalid_params)?;
    let mut matching = offers
        .iter()
        .filter(|offer| offer.get("id").and_then(Value::as_str) == Some(method_id));
    let selected = matching.next().ok_or_else(Error::invalid_params)?;
    let agent_type = match selected.get("type") {
        None => true,
        Some(Value::String(kind)) => kind == "agent",
        _ => false,
    };
    if matching.next().is_some() || !agent_type {
        return Err(Error::invalid_params());
    }
    let method: AuthMethod =
        serde_json::from_value(selected.clone()).map_err(|_| Error::invalid_params())?;
    let AuthMethod::Agent(method) = method else {
        return Err(Error::invalid_params());
    };
    Ok(Some(AuthenticateRequest::new(method.id)))
}

#[cfg(test)]
#[path = "provider_initialize_request_test.rs"]
mod tests;
