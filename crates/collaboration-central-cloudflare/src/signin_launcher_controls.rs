//! Private fixture clock and read-only observation; absent from production bundles.
use crate::workspace_central_object::CentralObject;
use fleet_identity_protocol::OrganizationId;
use serde::Deserialize;
use worker::{Response, Result};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ClockSample {
    milliseconds: Option<central_storage::SafeInteger>,
}

pub(crate) async fn sample_clock(
    object: &CentralObject,
    organization: &OrganizationId,
) -> Result<()> {
    // Absence preserves the existing per-object fixture clock contract.
    let Ok(source) = object.env.service("TEST_SIGNIN_CLOCK") else {
        return Ok(());
    };
    let mut response: Response = source
        .fetch(
            format!("https://signin-clock.test/clock/{}", organization.as_str()),
            None,
        )
        .await?
        .try_into()?;
    if response.status_code() != 200 {
        return Err(worker::Error::RustError(
            "fixture clock unavailable".to_owned(),
        ));
    }
    let sample: ClockSample = response.json().await?;
    if let Some(milliseconds) = sample.milliseconds {
        object.clock.hold(milliseconds.into());
    }
    Ok(())
}

pub(crate) async fn observe(
    object: &CentralObject,
    organization: &OrganizationId,
) -> Result<Response> {
    let observation =
        collaboration_central_core::signin_test_observation::observe_signin_organization(
            object.statement_host().as_ref(),
            organization,
        )
        .await
        .map_err(|_| worker::Error::RustError("fixture observation unavailable".to_owned()))?;
    Response::from_json(&observation)
}
