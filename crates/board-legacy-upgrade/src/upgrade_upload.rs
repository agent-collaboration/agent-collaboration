//! Re-running starts with central's durable identity and resends immutable pages.

use crate::{PreparedProject, PreparedUpgrade, ProjectImportReport, UpgradeError};
use board_sync_protocol::BoardAnnotation;
use collaboration_central_admin_protocol::OwnerOperationFailure;
use collaboration_central_core::{
    AdminImportHeader, AdminProjectImportAnswer, AdminProjectImportRequest,
};
use fleet_admission::FleetImportResult;
use fleet_sync_protocol::{PageDigest, SnapshotCursor};
use fleet_sync_writer::WriterSnapshot;
use message_board::{FeedPosition, FeedRead, ProjectId, Record};
use serde::{Serialize, de::DeserializeOwned};

#[derive(Debug, thiserror::Error)]
pub enum UpgradeUploadError {
    #[error("name collisions refuse publication")]
    NameCollisions,
    #[error("conflicting project content refuses publication")]
    ConflictingSources,
    #[error("prepared report and native snapshot disagree")]
    PreparedSnapshotMismatch,
    #[error("upload request failed; re-run the same source to resume: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("central owner operation failed: {failure:?}")]
    OwnerFailure { failure: OwnerOperationFailure },
    #[error("upload data is invalid: {0}")]
    Preparation(#[from] UpgradeError),
    #[error("central refused fleet import: {0:?}")]
    FleetRefused(FleetImportResult),
    #[error("central did not complete project {}: {answer:?}; re-run the same source to resume", project.as_str())]
    ProjectIncomplete {
        project: ProjectId,
        answer: Box<AdminProjectImportAnswer>,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpgradeUploadReport {
    pub fleet: FleetImportResult,
    pub projects: Vec<ProjectUploadReport>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectUploadReport {
    pub project: ProjectId,
    pub answer: AdminProjectImportAnswer,
}

/// The caller configures the existing owner Access credentials on this client.
pub struct UpgradePublisher {
    endpoint: reqwest::Url,
    client: reqwest::Client,
}
impl UpgradePublisher {
    pub fn new(endpoint: reqwest::Url, client: reqwest::Client) -> Self {
        Self { endpoint, client }
    }

    pub async fn publish(
        &self,
        prepared: &PreparedUpgrade,
    ) -> Result<UpgradeUploadReport, UpgradeUploadError> {
        prepared.ensure_publishable()?;
        let fleet: FleetImportResult = self
            .post("/admin/v1/import/fleet", &prepared.report.fleet)
            .await?;
        if !matches!(
            fleet,
            FleetImportResult::Imported { .. } | FleetImportResult::AlreadyImported
        ) {
            return Err(UpgradeUploadError::FleetRefused(fleet));
        }
        let mut projects = Vec::new();
        for (project, report) in prepared.projects.iter().zip(&prepared.report.projects) {
            let answer = self.publish_project(project, report, prepared).await?;
            projects.push(ProjectUploadReport {
                project: report.project.clone(),
                answer,
            });
        }
        Ok(UpgradeUploadReport { fleet, projects })
    }

    async fn publish_project(
        &self,
        project: &PreparedProject,
        report: &ProjectImportReport,
        prepared: &PreparedUpgrade,
    ) -> Result<AdminProjectImportAnswer, UpgradeUploadError> {
        let mut snapshot = project
            .database
            .snapshot()
            .await
            .map_err(UpgradeError::from)?;
        let outcome = self
            .publish_snapshot(project, report, prepared, &mut snapshot)
            .await;
        let closed = snapshot.close().await.map_err(UpgradeError::from);
        // Preserve a publication failure even if releasing its reader also fails.
        let answer = outcome?;
        closed?;
        Ok(answer)
    }

    async fn publish_snapshot(
        &self,
        project: &PreparedProject,
        report: &ProjectImportReport,
        prepared: &PreparedUpgrade,
        snapshot: &mut WriterSnapshot,
    ) -> Result<AdminProjectImportAnswer, UpgradeUploadError> {
        if snapshot.manifest().await.map_err(UpgradeError::from)? != report.manifest {
            return Err(UpgradeUploadError::PreparedSnapshotMismatch);
        }
        // Count via the library's paging API, then stream those pages from the
        // same pinned read. This avoids buffering a whole project or copying a
        // private library page-size constant into the importer.
        let mut pages = 0u32;
        let mut cursor = None;
        loop {
            let page = snapshot.page(cursor).await.map_err(UpgradeError::from)?;
            pages = pages
                .checked_add(1)
                .ok_or(UpgradeUploadError::PreparedSnapshotMismatch)?;
            cursor = page.next;
            if cursor.is_none() {
                break;
            }
        }
        let header = AdminImportHeader {
            digest: report.digest.clone(),
            provenance: prepared.report.fleet.provenance.clone(),
            declared_owner: prepared.report.owner.clone(),
            manifest: report.manifest.clone(),
            pages,
            summary: summary(project, &report.project).await?,
        };
        let answer = self
            .project(
                &report.project,
                &AdminProjectImportRequest::Begin { header },
            )
            .await?;
        let import_id = match answer {
            AdminProjectImportAnswer::Ready { import_id, .. } => import_id,
            AdminProjectImportAnswer::AlreadyImported => return Ok(answer),
            _ => return Err(incomplete(report, answer)),
        };
        let mut cursor: Option<SnapshotCursor> = None;
        for index in 0..pages {
            let contents = snapshot.page(cursor).await.map_err(UpgradeError::from)?;
            cursor = contents.next.clone();
            let page_digest = PageDigest::of(&contents)
                .map_err(|error| UpgradeError::Snapshot(error.to_string()))?;
            let answer = self
                .project(
                    &report.project,
                    &AdminProjectImportRequest::Page {
                        digest: report.digest.clone(),
                        import_id,
                        page: index,
                        contents,
                        page_digest,
                    },
                )
                .await?;
            if answer
                != (AdminProjectImportAnswer::Staged {
                    import_id,
                    page: index,
                })
            {
                return Err(incomplete(report, answer));
            }
        }
        if cursor.is_some() {
            return Err(UpgradeUploadError::PreparedSnapshotMismatch);
        }
        let answer = self
            .project(
                &report.project,
                &AdminProjectImportRequest::Complete {
                    digest: report.digest.clone(),
                    import_id,
                },
            )
            .await?;
        match answer {
            AdminProjectImportAnswer::Imported { head } if head == report.manifest.position => {
                Ok(answer)
            }
            AdminProjectImportAnswer::AlreadyImported => Ok(answer),
            _ => Err(incomplete(report, answer)),
        }
    }

    async fn project(
        &self,
        project: &ProjectId,
        request: &AdminProjectImportRequest,
    ) -> Result<AdminProjectImportAnswer, UpgradeUploadError> {
        self.post(
            &format!("/admin/v1/import/projects/{}", project.as_str()),
            request,
        )
        .await
    }
    async fn post<Input: Serialize + ?Sized, Output: DeserializeOwned>(
        &self,
        path: &str,
        input: &Input,
    ) -> Result<Output, UpgradeUploadError> {
        let mut endpoint = self.endpoint.clone();
        endpoint.set_path(path);
        let response = self.client.post(endpoint).json(input).send().await?;
        if matches!(
            response.status(),
            reqwest::StatusCode::GONE | reqwest::StatusCode::SERVICE_UNAVAILABLE
        ) {
            return Err(UpgradeUploadError::OwnerFailure {
                failure: response.json().await?,
            });
        }
        let response = response.error_for_status()?;
        Ok(response.json().await?)
    }
}

impl PreparedUpgrade {
    /// A collision-bearing run must fail before acquiring credentials or making IO.
    pub fn ensure_publishable(&self) -> Result<(), UpgradeUploadError> {
        if !self.report.collisions.is_empty() {
            return Err(UpgradeUploadError::NameCollisions);
        }
        if !self.report.refused.is_empty() {
            return Err(UpgradeUploadError::ConflictingSources);
        }
        if self.projects.len() != self.report.projects.len() {
            return Err(UpgradeUploadError::PreparedSnapshotMismatch);
        }
        Ok(())
    }
}

fn incomplete(
    report: &ProjectImportReport,
    answer: AdminProjectImportAnswer,
) -> UpgradeUploadError {
    UpgradeUploadError::ProjectIncomplete {
        project: report.project.clone(),
        answer: Box::new(answer),
    }
}

async fn summary(
    project: &PreparedProject,
    id: &ProjectId,
) -> Result<BoardAnnotation, UpgradeUploadError> {
    let mut snapshot = project.store.snapshot().await.map_err(UpgradeError::from)?;
    let outcome = async {
        let Some(Record::Project(state)) = snapshot.record(&id.clone().into()).await? else {
            return Err(UpgradeError::Snapshot(
                "imported ProjectState absent".into(),
            ));
        };
        let high = snapshot.high_water();
        let before = high
            .position
            .get()
            .checked_sub(1)
            .ok_or_else(|| UpgradeError::Snapshot("imported feed empty".into()))?;
        let position = FeedPosition::try_from(before)
            .map_err(|error| UpgradeError::Snapshot(error.to_string()))?;
        let after = snapshot.feed_cursor_or_start(position).await?;
        let FeedRead::Entries {
            entries,
            covered_through,
            ..
        } = snapshot.feed_after(after, 1, true).await?
        else {
            return Err(UpgradeError::Snapshot("imported feed superseded".into()));
        };
        let latest = entries
            .last()
            .ok_or_else(|| UpgradeError::Snapshot("imported latest record absent".into()))?;
        if covered_through != high {
            return Err(UpgradeError::Snapshot(
                "imported summary cutoff differs".into(),
            ));
        }
        Ok(BoardAnnotation::ProjectSummary {
            status: state.status,
            latest_record_at: latest.time,
        })
    }
    .await;
    let closed = snapshot.finish().await.map_err(UpgradeError::from);
    let annotation = outcome?;
    closed?;
    Ok(annotation)
}
