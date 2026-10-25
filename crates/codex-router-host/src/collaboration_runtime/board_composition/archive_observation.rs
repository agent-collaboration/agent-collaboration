//! Fleet state prompts local archive admission; only native Copy acknowledgement permits the report.
use super::project_writers::ProjectWriters;
use collaboration_service::{FleetClient, ProjectAdmission, ProjectArchiveObservation};
use message_board::ProjectId;
use std::{
    io,
    sync::{Arc, Weak},
};

pub(super) async fn synchronize_and_observe(
    projects: &Arc<ProjectWriters>,
    admission: &Weak<ProjectAdmission>,
    fleet: &Weak<FleetClient>,
) -> io::Result<()> {
    let fleet = fleet
        .upgrade()
        .ok_or_else(|| io::Error::other("fleet client has closed"))?;
    fleet.synchronize().await.map_err(io::Error::other)?;
    let admission = admission
        .upgrade()
        .ok_or_else(|| io::Error::other("project admission has closed"))?;
    for project in projects
        .registry()?
        .project_ids()
        .await
        .map_err(io::Error::other)?
    {
        if let Err(error) = observe_project(projects, &admission, &fleet, &project).await {
            tracing::warn!(?project, %error, "held-project archive observation unavailable");
        }
    }
    Ok(())
}

pub(super) async fn observe_acknowledged_project(
    projects: &Arc<ProjectWriters>,
    admission: &Weak<ProjectAdmission>,
    fleet: &Weak<FleetClient>,
    project: &ProjectId,
) -> io::Result<()> {
    let admission = admission
        .upgrade()
        .ok_or_else(|| io::Error::other("project admission has closed"))?;
    let fleet = fleet
        .upgrade()
        .ok_or_else(|| io::Error::other("fleet client has closed"))?;
    observe_project(projects, &admission, &fleet, project).await
}

async fn observe_project(
    projects: &ProjectWriters,
    admission: &ProjectAdmission,
    fleet: &FleetClient,
    project: &ProjectId,
) -> io::Result<()> {
    let Some(observation) = admission
        .observe_archiving_project(project)
        .await
        .map_err(io::Error::other)?
    else {
        return Ok(());
    };
    report_observation(projects, fleet, project, observation).await
}

pub(super) async fn report_observation(
    projects: &ProjectWriters,
    fleet: &FleetClient,
    project: &ProjectId,
    observation: ProjectArchiveObservation,
) -> io::Result<()> {
    let ProjectArchiveObservation {
        store: observed_store,
        board,
        installation,
        position,
    } = observation;
    let (store, copied) = projects
        .acknowledged_project_copy(project)
        .await
        .map_err(io::Error::other)?;
    if !Arc::ptr_eq(&observed_store, &store) {
        // A later scan observes the replacement itself; its progress cannot prove this old fact.
        return Ok(());
    }
    let Some(copied) = copied else {
        return Ok(());
    };
    let through = collaboration_service::project_replication_cursor(&store, copied, position.epoch)
        .await
        .map_err(io::Error::other)?;
    if through.epoch < position.epoch || through.position < position.position {
        return Ok(());
    }
    let outcome = fleet
        .report_archive_seen(fleet_admission::ArchiveReport {
            board,
            project: project.clone(),
            position: copied,
            installation,
            reported_at: chrono::Utc::now(),
        })
        .await
        .map_err(io::Error::other)?;
    match outcome {
        fleet_admission::ArchiveReportOutcome::Recorded
        | fleet_admission::ArchiveReportOutcome::AlreadyRecorded => Ok(()),
        fleet_admission::ArchiveReportOutcome::Refused(reason) => Err(io::Error::other(format!(
            "archive report refused: {reason:?}"
        ))),
    }
}
