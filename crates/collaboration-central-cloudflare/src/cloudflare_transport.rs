//! Durable Object addressing and byte transport, without caller policy.

use collaboration_central_core::ObjectAddress;
use durable_steps::{CallFailure, NotSentReason};
use worker::*;

// The library's transport error deliberately has no Serde implementation.
// This private HTTP codec preserves every outcome and diagnostic at this edge.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum WireCallFailure {
    Unreachable {
        last_seen: Option<runtime_primitives::SyncTime>,
    },
    Overloaded,
    AnswerLost,
    Retryable(durable_steps::CallRetryReason),
}
impl From<CallFailure> for WireCallFailure {
    fn from(failure: CallFailure) -> Self {
        match failure {
            CallFailure::NotSent {
                reason: NotSentReason::Unreachable { last_seen },
            } => Self::Unreachable { last_seen },
            CallFailure::NotSent {
                reason: NotSentReason::Overloaded,
            } => Self::Overloaded,
            CallFailure::AnswerLost => Self::AnswerLost,
            CallFailure::Retryable { reason } => Self::Retryable(reason),
        }
    }
}
impl From<WireCallFailure> for CallFailure {
    fn from(failure: WireCallFailure) -> Self {
        match failure {
            WireCallFailure::Unreachable { last_seen } => Self::NotSent {
                reason: NotSentReason::Unreachable { last_seen },
            },
            WireCallFailure::Overloaded => Self::NotSent {
                reason: NotSentReason::Overloaded,
            },
            WireCallFailure::AnswerLost => Self::AnswerLost,
            WireCallFailure::Retryable(reason) => Self::Retryable { reason },
        }
    }
}

pub(crate) use crate::object_location::location;
pub(crate) fn stub(env: &Env, address: &ObjectAddress) -> Result<Stub> {
    let (binding, name) = location(address);
    env.durable_object(binding)?.id_from_name(&name)?.get_stub()
}
pub(crate) fn address_header(headers: &Headers, address: &ObjectAddress) -> Result<()> {
    headers.set("x-central-object-address", &serde_json::to_string(address)?)
}
pub(crate) async fn call(
    env: &Env,
    address: ObjectAddress,
    bytes: Vec<u8>,
) -> std::result::Result<Vec<u8>, CallFailure> {
    let stub = stub(env, &address).map_err(|_| unreachable())?;
    let headers = Headers::new();
    address_header(&headers, &address).map_err(|_| unreachable())?;
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(js_sys::Uint8Array::from(bytes.as_slice()).into()));
    let request = Request::new_with_init("https://central.internal/call", &init)
        .map_err(|_| unreachable())?;
    let mut response = stub
        .fetch_with_request(request)
        .await
        .map_err(|_| CallFailure::AnswerLost)?;
    match response.status_code() {
        200 => response.bytes().await.map_err(|_| CallFailure::AnswerLost),
        409 => Err(response
            .json::<WireCallFailure>()
            .await
            .map_err(|_| CallFailure::AnswerLost)?
            .into()),
        _ => Err(CallFailure::AnswerLost),
    }
}
fn unreachable() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}
