//! Opt-in, redacted evidence for one controlled native client; no wire or policy changes.

use crate::app_server_model_catalog::ProviderModelEntry;
use serde_json::Value;

const TARGET: &str = "collaboration_runtime::native_settings_diagnostic";
const MAX_MODEL_BYTES: usize = 128;

pub(super) enum Reply<'a> {
    Response(&'a Value),
    CatalogUnavailable(&'a Value),
    DispatchUnavailable,
}

#[derive(Debug, Eq, PartialEq)]
enum Presence {
    Absent,
    Null,
    Value,
}

#[derive(Debug, Eq, PartialEq)]
enum Token<'a> {
    Absent,
    Null,
    Known(&'a str),
    Redacted,
}

impl Token<'_> {
    fn state(&self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Null => "null",
            Self::Known(_) => "value",
            Self::Redacted => "redacted",
        }
    }

    fn value(&self) -> Option<&str> {
        match self {
            Self::Known(value) => Some(value),
            Self::Absent | Self::Null | Self::Redacted => None,
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
enum Outcome {
    Success,
    Error(i64),
    CatalogUnavailable(Option<i64>),
    DispatchUnavailable,
    MalformedReply,
}

impl Outcome {
    fn class(&self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Error(-32602) => "invalid_params",
            Self::Error(-32002) => "not_found",
            Self::Error(-32600) => "invalid_request",
            Self::Error(_) => "server_error",
            Self::CatalogUnavailable(_) => "catalog_unavailable",
            Self::DispatchUnavailable => "dispatch_unavailable",
            Self::MalformedReply => "malformed_reply",
        }
    }

    fn code(&self) -> Option<i64> {
        match self {
            Self::Error(code) => Some(*code),
            Self::CatalogUnavailable(code) => *code,
            Self::Success | Self::DispatchUnavailable | Self::MalformedReply => None,
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
enum Projection<'a> {
    Settings {
        request_id: u64,
        outcome: Outcome,
        params: Presence,
        model: Token<'a>,
        effort: Token<'a>,
        collaboration_mode: Presence,
        default_mode: bool,
        settings: Presence,
        nested_model: Token<'a>,
        nested_effort: Token<'a>,
        developer_instructions: Presence,
        service_tier: Presence,
    },
    DefaultSave {
        request_id_kind: &'static str,
        outcome: Outcome,
    },
}

fn presence(value: Option<&Value>) -> Presence {
    match value {
        None => Presence::Absent,
        Some(Value::Null) => Presence::Null,
        Some(_) => Presence::Value,
    }
}

fn token<'a>(value: Option<&'a Value>, allowed: impl FnOnce(&str) -> bool) -> Token<'a> {
    match value {
        None => Token::Absent,
        Some(Value::Null) => Token::Null,
        Some(Value::String(value)) if allowed(value) => Token::Known(value),
        Some(_) => Token::Redacted,
    }
}

fn model<'a>(value: Option<&'a Value>, catalog: &[ProviderModelEntry]) -> Token<'a> {
    token(value, |value| {
        value.len() <= MAX_MODEL_BYTES
            && value.bytes().all(|byte| byte.is_ascii_graphic())
            && catalog.iter().any(|entry| entry.id() == value)
    })
}

fn effort(value: Option<&Value>) -> Token<'_> {
    token(value, |value| {
        matches!(
            value,
            "provider"
                | "default"
                | "none"
                | "minimal"
                | "low"
                | "medium"
                | "high"
                | "xhigh"
                | "max"
                | "ultra"
                | "persistent"
        )
    })
}

fn project<'a>(
    request: &'a Value,
    reply: Reply<'_>,
    catalog: &[ProviderModelEntry],
) -> Option<Projection<'a>> {
    let method = request.get("method")?.as_str()?;
    if !matches!(method, "thread/settings/update" | "config/batchWrite") {
        return None;
    }
    let request_id = request.get("id")?;
    let outcome = match reply {
        Reply::Response(response) | Reply::CatalogUnavailable(response) => {
            if response.get("id") != request.get("id") {
                return None;
            }
            let code = response.pointer("/error/code").and_then(Value::as_i64);
            if matches!(reply, Reply::CatalogUnavailable(_)) {
                Outcome::CatalogUnavailable(code)
            } else if let Some(code) = code {
                Outcome::Error(code)
            } else if response.get("result").is_some() && response.get("error").is_none() {
                Outcome::Success
            } else {
                Outcome::MalformedReply
            }
        }
        Reply::DispatchUnavailable => Outcome::DispatchUnavailable,
    };
    if method == "config/batchWrite" {
        let request_id_kind = if request_id.is_string() {
            "string"
        } else if request_id.as_u64().is_some() {
            "integer"
        } else {
            return None;
        };
        return Some(Projection::DefaultSave {
            request_id_kind,
            outcome,
        });
    }
    let params = request.get("params");
    let field = |name| params.and_then(|params| params.get(name));
    let mode = field("collaborationMode");
    let settings = mode.and_then(|mode| mode.get("settings"));
    Some(Projection::Settings {
        request_id: request_id.as_u64()?,
        outcome,
        params: presence(params),
        model: model(field("model"), catalog),
        effort: effort(field("effort")),
        collaboration_mode: presence(mode),
        default_mode: mode
            .and_then(|mode| mode.get("mode"))
            .and_then(Value::as_str)
            == Some("default"),
        settings: presence(settings),
        nested_model: model(settings.and_then(|settings| settings.get("model")), catalog),
        nested_effort: effort(settings.and_then(|settings| settings.get("reasoning_effort"))),
        developer_instructions: presence(
            settings.and_then(|settings| settings.get("developer_instructions")),
        ),
        service_tier: presence(field("serviceTier")),
    })
}

/// Numeric settings IDs correlate this single-controlled-writer diagnostic only.
/// Opaque config-write IDs are matched in memory and never emitted. This is not
/// a general concurrent-client recorder or a durable identity.
pub(super) fn emit(request: &Value, reply: Reply<'_>, catalog: &[ProviderModelEntry]) {
    if !tracing::enabled!(target: TARGET, tracing::Level::DEBUG) {
        return;
    }
    let Some(projection) = project(request, reply, catalog) else {
        return;
    };
    match projection {
        Projection::DefaultSave {
            request_id_kind,
            outcome,
        } => tracing::debug!(target: TARGET, {
            event.name = "codex_router.native_settings_diagnostic", scope = "single_controlled_writer",
            method = "config/batchWrite", request_id_kind, outcome = outcome.class(), error_code = ?outcome.code(),
            },
            "Observed native compatibility outcome"),
        Projection::Settings {
            request_id,
            outcome,
            params,
            model,
            effort,
            collaboration_mode,
            default_mode,
            settings,
            nested_model,
            nested_effort,
            developer_instructions,
            service_tier,
        } => tracing::debug!(target: TARGET, {
            event.name = "codex_router.native_settings_diagnostic", scope = "single_controlled_writer",
            method = "thread/settings/update", request_id, outcome = outcome.class(), error_code = ?outcome.code(),
            params_state = ?params, model_state = model.state(), model = model.value(),
            effort_state = effort.state(), effort = effort.value(),
            collaboration_mode_state = ?collaboration_mode, default_mode, settings_state = ?settings,
            nested_model_state = nested_model.state(), nested_model = nested_model.value(),
            nested_effort_state = nested_effort.state(), nested_effort = nested_effort.value(),
            developer_instructions_state = ?developer_instructions, service_tier_state = ?service_tier,
            },
            "Observed native compatibility outcome"),
    }
}

#[cfg(test)]
#[path = "native_settings_diagnostics_test.rs"]
mod tests;
