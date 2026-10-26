//! One bounded owner enrollment request; uncertain replies are never retried.
use super::{FleetCommandError, enrollment_prepare::PreparedEnrollment};
use collaboration_central_core::{EnrollMachine, EnrollmentDecision};
use serde::Serialize;
use std::time::Duration;

#[derive(Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
enum EnrollmentBody<'request> {
    EnrollMachine(&'request EnrollMachine),
}

pub(super) async fn enroll(
    prepared: &PreparedEnrollment,
) -> Result<EnrollmentDecision, FleetCommandError> {
    let client = reqwest::Client::builder()
        .http1_only()
        .retry(reqwest::retry::never())
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(45))
        .build()
        .map_err(|_| {
            FleetCommandError::InvalidInput("could not initialize central enrollment transport")
        })?;
    let mut response = client
        .post(prepared.admin_url.clone())
        .header(prepared.owner_header, prepared.owner_token.clone())
        .json(&EnrollmentBody::EnrollMachine(&prepared.request))
        .send()
        .await
        .map_err(|_| {
            FleetCommandError::OutcomeUnknown("the owner request did not yield a response")
        })?;
    if response.status() != reqwest::StatusCode::OK {
        return Err(FleetCommandError::UnexpectedStatus(
            response.status().as_u16(),
        ));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| FleetCommandError::OutcomeUnknown("the owner response was interrupted"))?
    {
        if body.len() + chunk.len() > 32768 {
            return Err(FleetCommandError::OutcomeUnknown(
                "the owner response exceeded its bound",
            ));
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|_| {
        FleetCommandError::OutcomeUnknown("the owner response was not a valid EnrollmentDecision")
    })
}
