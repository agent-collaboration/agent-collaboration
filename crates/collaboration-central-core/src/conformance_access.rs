//! Owner-gated proof of the same verifier used by the normal HTTP edge.

use crate::{AccessCaller, AccessRefusal, CentralApplication, CentralPlatform};
use axum::{
    Extension, Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum AccessVerificationOutcome {
    Accepted(AccessCallerClass),
    Refused(AccessRefusal),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AccessCallerClass {
    Machine,
    Owner,
}

impl AccessVerificationOutcome {
    #[must_use]
    pub fn of(verified: Result<crate::AccessCaller, AccessRefusal>) -> Self {
        match verified {
            Ok(crate::AccessCaller::Machine { .. }) => Self::Accepted(AccessCallerClass::Machine),
            Ok(crate::AccessCaller::Owner { .. }) => Self::Accepted(AccessCallerClass::Owner),
            Err(refusal) => Self::Refused(refusal),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VerifyAccessInput {
    assertion: String,
    path: String,
    now: Option<crate::RegistryTime>,
}

pub(crate) async fn verify<Platform: CentralPlatform>(
    State(application): State<Arc<CentralApplication<Platform>>>,
    Extension(caller): Extension<AccessCaller>,
    Json(input): Json<VerifyAccessInput>,
) -> Response {
    if !matches!(caller, AccessCaller::Owner { .. }) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if let Some(now) = input.now {
        let Some(clock) = application.platform.conformance_access_clock() else {
            return StatusCode::CONFLICT.into_response();
        };
        clock.set(runtime_primitives::SyncTime::from_unix_millis(
            now.0.timestamp_millis(),
        ));
    }
    Json(AccessVerificationOutcome::of(
        application
            .verify_access(Some(&input.assertion), &input.path)
            .await,
    ))
    .into_response()
}
