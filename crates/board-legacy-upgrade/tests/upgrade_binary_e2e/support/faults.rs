use super::{CentralHarness, E2eResult, promote_error, promote_result};
use crate::{central_fixture, upload_support};
use board_legacy_upgrade::PreparedUpgrade;
use collaboration_central_core::{
    AdminImportPhase, AdminProjectImportAnswer, AdminProjectImportRequest,
};
use std::path::Path;

pub async fn reserve_first_project(
    central: &CentralHarness,
    prepared: &PreparedUpgrade,
) -> E2eResult<(collaboration_central_core::AdminImportHeader, String)> {
    let header = promote_result(upload_support::header(prepared).await)?;
    let project = prepared
        .report
        .projects
        .first()
        .ok_or_else(|| promote_error("prepared project list is empty"))?
        .project
        .as_str()
        .to_owned();
    let client = promote_result(central_fixture::owner_client())?;
    let response = client
        .post(format!(
            "{}/admin/v1/import/projects/{project}",
            central.base_url()?
        ))
        .json(&AdminProjectImportRequest::Begin {
            header: header.clone(),
        })
        .send()
        .await?
        .error_for_status()?;
    let answer: AdminProjectImportAnswer = response.json().await?;
    if !matches!(answer, AdminProjectImportAnswer::Ready { .. }) {
        return Err(promote_error(format!(
            "initial project reservation returned {answer:?}"
        )));
    }
    Ok((header, project))
}

pub async fn import_status(
    central: &CentralHarness,
    project: &str,
    digest: fleet_sync_protocol::ImportDigest,
) -> E2eResult<AdminImportPhase> {
    let client = promote_result(central_fixture::owner_client())?;
    let response = client
        .post(format!(
            "{}/admin/v1/import/projects/{project}",
            central.base_url()?
        ))
        .json(&AdminProjectImportRequest::Status { digest })
        .send()
        .await?
        .error_for_status()?;
    let answer: AdminProjectImportAnswer = response.json().await?;
    let AdminProjectImportAnswer::Progress(progress) = answer else {
        return Err(promote_error(format!(
            "project status was not retained: {answer:?}"
        )));
    };
    Ok(progress.phase)
}

pub async fn install_prepublication_fault(root: &Path, copy: &str) -> E2eResult<()> {
    promote_result(upload_support::execute(
        root,
        copy,
        "CREATE TRIGGER interrupt_import BEFORE INSERT ON _copy_summary BEGIN SELECT RAISE(ABORT,'publication interrupted'); END",
    )
    .await)
}

pub async fn install_completion_fault(root: &Path) -> E2eResult<()> {
    promote_result(upload_support::execute(
        root,
        "fleet",
        "CREATE TRIGGER interrupt_import BEFORE UPDATE OF phase ON admin_imports WHEN json_extract(NEW.phase,'$.kind')='complete' BEGIN SELECT RAISE(ABORT,'completion receipt interrupted'); END",
    )
    .await)
}

pub async fn remove_fault(root: &Path, object: &str) -> E2eResult<()> {
    promote_result(upload_support::execute(root, object, "DROP TRIGGER interrupt_import").await)
}
