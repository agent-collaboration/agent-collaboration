//! One behavioral contract for the volatile stand-in and the future real fleet store.
use crate::{
    fleet_login::{FleetLoginCandidate, SavedFleetCentral, SavedFleetLogin},
    fleet_login_repository::{
        FleetLoginRepository, FleetLoginStorageFailure, FleetLoginStorageRequest,
    },
};
use fleet_identity_protocol::MachineEnvironment;
use std::{future::poll_fn, task::Poll};

#[derive(Debug, thiserror::Error)]
pub enum FleetLoginContractFailure {
    #[error(transparent)]
    Storage(#[from] FleetLoginStorageFailure),
    #[error("invalid repository contract fixture")]
    Fixture,
    #[error("fleet login repository contract failed: {0}")]
    Violation(&'static str),
}

fn require(condition: bool, description: &'static str) -> Result<(), FleetLoginContractFailure> {
    if condition {
        Ok(())
    } else {
        Err(FleetLoginContractFailure::Violation(description))
    }
}

pub(super) fn candidate_fixture(
    ordinal: u16,
    environment: Option<MachineEnvironment>,
) -> Result<FleetLoginCandidate, FleetLoginContractFailure> {
    Ok(FleetLoginCandidate {
        sign_in: format!("https://issuer.example.invalid/{ordinal}")
            .try_into()
            .map_err(|_| FleetLoginContractFailure::Fixture)?,
        organization_id: "019f0000-0000-7000-8000-000000000001"
            .to_owned()
            .try_into()
            .map_err(|_| FleetLoginContractFailure::Fixture)?,
        enrollment_id: format!("019f0000-0000-7000-8000-{ordinal:012x}")
            .try_into()
            .map_err(|_| FleetLoginContractFailure::Fixture)?,
        requested_machine_environment: environment,
        central: SavedFleetCentral {
            receiver: serde_json::from_value(serde_json::json!(format!(
                "019f0000-0000-7000-8000-{:012x}",
                u32::from(ordinal) + 0x100
            )))
            .map_err(|_| FleetLoginContractFailure::Fixture)?,
            relay: serde_json::from_value(serde_json::json!(format!(
                "019f0000-0000-7000-8000-{:012x}",
                u32::from(ordinal) + 0x200
            )))
            .map_err(|_| FleetLoginContractFailure::Fixture)?,
        },
    })
}

pub(super) fn expected_login(
    candidate: &FleetLoginCandidate,
    environment: MachineEnvironment,
) -> SavedFleetLogin {
    SavedFleetLogin {
        sign_in: candidate.sign_in.clone(),
        organization_id: candidate.organization_id.clone(),
        enrollment_id: candidate.enrollment_id.clone(),
        machine_environment: environment,
        central: candidate.central.clone(),
    }
}

async fn concurrent_replacements(
    mut left: FleetLoginStorageRequest<'_, SavedFleetLogin>,
    mut right: FleetLoginStorageRequest<'_, SavedFleetLogin>,
) -> Result<(SavedFleetLogin, SavedFleetLogin), FleetLoginContractFailure> {
    let mut left_result = None;
    let mut right_result = None;
    let (left, right) = poll_fn(move |context| {
        if left_result.is_none()
            && let Poll::Ready(result) = left.as_mut().poll(context)
        {
            left_result = Some(result);
        }
        if right_result.is_none()
            && let Poll::Ready(result) = right.as_mut().poll(context)
        {
            right_result = Some(result);
        }
        if left_result.is_some() && right_result.is_some() {
            match (left_result.take(), right_result.take()) {
                (Some(left), Some(right)) => Poll::Ready((left, right)),
                _ => Poll::Pending,
            }
        } else {
            Poll::Pending
        }
    })
    .await;
    Ok((left?, right?))
}

/// Run on a fresh empty repository. No database implementation is accepted
/// until it passes this same contract; stand-in success proves no durability.
pub async fn verify_login_repository_contract(
    repository: &dyn FleetLoginRepository,
) -> Result<(), FleetLoginContractFailure> {
    require(
        repository.load_fleet_login().await?.is_none(),
        "fresh repository must be empty",
    )?;
    for (ordinal, requested, confirmed) in [
        (0x10, None, MachineEnvironment::Physical),
        (
            0x11,
            Some(MachineEnvironment::CloudVm),
            MachineEnvironment::CloudVm,
        ),
        (0x12, None, MachineEnvironment::CloudVm),
        (
            0x13,
            Some(MachineEnvironment::Physical),
            MachineEnvironment::Physical,
        ),
    ] {
        let candidate = candidate_fixture(ordinal, requested)?;
        let expected = expected_login(&candidate, confirmed);
        let saved = repository.replace_fleet_login(&candidate).await?;
        require(
            saved == expected,
            "replacement must return the confirmed whole row and environment",
        )?;
        require(
            repository.load_fleet_login().await? == Some(expected),
            "readback must match the confirmed replacement",
        )?;
    }

    let retained = repository.load_fleet_login().await?;
    let mut foreign = candidate_fixture(0x14, Some(MachineEnvironment::CloudVm))?;
    foreign.organization_id = "019f0000-0000-7000-8000-000000000009"
        .to_owned()
        .try_into()
        .map_err(|_| FleetLoginContractFailure::Fixture)?;
    require(
        matches!(
            repository.replace_fleet_login(&foreign).await,
            Err(FleetLoginStorageFailure::OrganizationMismatch { .. })
        ),
        "another organization must not replace the row",
    )?;
    require(
        repository.load_fleet_login().await? == retained,
        "organization refusal must preserve every field",
    )?;

    let stale = candidate_fixture(0x11, None)?;
    require(
        !repository.delete_fleet_login(&stale.enrollment_id).await?,
        "stale enrollment must not delete a newer row",
    )?;
    require(
        repository.load_fleet_login().await? == retained,
        "stale delete must preserve every field",
    )?;

    let left = candidate_fixture(0x20, Some(MachineEnvironment::CloudVm))?;
    let right = candidate_fixture(0x21, None)?;
    let (left_saved, right_saved) = concurrent_replacements(
        repository.replace_fleet_login(&left),
        repository.replace_fleet_login(&right),
    )
    .await?;
    let left_expected = expected_login(&left, MachineEnvironment::CloudVm);
    let right_after_left = expected_login(&right, MachineEnvironment::CloudVm);
    let right_before_left = expected_login(&right, MachineEnvironment::Physical);
    let final_row = repository.load_fleet_login().await?;
    require(
        left_saved == left_expected,
        "explicit racing environment must be confirmed",
    )?;
    require(
        (right_saved == right_before_left && final_row == Some(left_expected))
            || (right_saved == right_after_left && final_row == Some(right_after_left)),
        "racing replacements must have one whole-row serial order, including environment retention",
    )?;
    let final_row = final_row.ok_or(FleetLoginContractFailure::Violation(
        "racing row is missing",
    ))?;
    require(
        repository
            .delete_fleet_login(&final_row.enrollment_id)
            .await?,
        "matching enrollment must delete its row",
    )?;
    require(
        repository.load_fleet_login().await?.is_none(),
        "confirmed delete must leave no row",
    )?;
    Ok(())
}
