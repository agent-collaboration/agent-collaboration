//! Central's import workflow owns publication identity and completion.
use crate::owner_publication_access::{
    OwnerPublicationAccessError, build_owner_client, headers_from_private_file, owner_access_client,
};
use crate::{OfflineHost, PreparedProjectBundle, ProjectBundleError};
use collaboration_central_admin_protocol::OwnerOperationFailure;
use collaboration_central_core::{
    AdminImportPhase, AdminImportRecord, AdminImportRefusal, AdminProjectImportAnswer,
    AdminProjectImportRequest,
};
use fleet_admission::FleetImportResult;
use fleet_sync_protocol::{ReplicationPosition, RequestId};
use message_board::CanonicalHash;
use runtime_primitives::SystemClock;
use serde::{Serialize, de::DeserializeOwned};
use std::path::Path;
use url::Url;

#[cfg(test)]
#[path = "staging_publication_test.rs"]
mod tests;

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PublicationStage {
    Fleet,
    Status,
    Begin,
    Page { index: u32 },
    Complete,
}

#[derive(Debug, thiserror::Error)]
pub enum PublicationError {
    #[error("publisher and prepared bundle have different central origins")]
    TargetMismatch,
    #[error("the prepared source belongs to another offline Host")]
    SourceHostMismatch,
    #[error("the source directory changed after immutable preparation")]
    SourceChanged,
    #[error("selected native source is unavailable: {0}")]
    Source(#[from] crate::NativeSourceError),
    #[error("central status differs from the prepared import")]
    StatusMismatch,
    #[error("central did not finish project publication")]
    Incomplete {
        answer: Box<AdminProjectImportAnswer>,
    },
    #[error("central refused the preserved Fleet graph")]
    FleetRefused { answer: Box<FleetImportResult> },
    #[error("the owner route failed")]
    OwnerFailure { failure: Box<OwnerOperationFailure> },
    #[error("publication request outcome is unknown; reconcile the same immutable bundle")]
    OutcomeUnknown {
        stage: PublicationStage,
        source: reqwest::Error,
    },
    #[error("owner publication access is unavailable")]
    Access(#[from] OwnerPublicationAccessError),
    #[error("prepared bundle is invalid")]
    Bundle(#[from] ProjectBundleError),
    #[error("cannot encode the prepared request")]
    Json(#[from] serde_json::Error),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicationDiagnostic {
    stage: PublicationStage,
    failure: PublicationRequestFailure,
    #[serde(skip_serializing_if = "Option::is_none")]
    io: Option<PublicationIoFailure>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum PublicationRequestFailure {
    Timeout { during: PublicationTimeoutContext },
    HttpStatus { status: u16 },
    Connect,
    Decode,
    Body,
    Request,
    Builder,
    Redirect,
    Other,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
enum PublicationTimeoutContext {
    Connect,
    Body,
    Request,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicationIoFailure {
    kind: String,
    os_error: Option<i32>,
}

impl PublicationError {
    pub fn diagnostic(&self) -> Option<PublicationDiagnostic> {
        match self {
            Self::OutcomeUnknown { stage, source } => Some(PublicationDiagnostic {
                stage: *stage,
                failure: PublicationRequestFailure::classify(source),
                io: safe_io_cause(source),
            }),
            Self::TargetMismatch
            | Self::SourceHostMismatch
            | Self::SourceChanged
            | Self::Source(_)
            | Self::StatusMismatch
            | Self::Incomplete { .. }
            | Self::FleetRefused { .. }
            | Self::OwnerFailure { .. }
            | Self::Access(_)
            | Self::Bundle(_)
            | Self::Json(_) => None,
        }
    }
}

impl PublicationRequestFailure {
    fn classify(error: &reqwest::Error) -> Self {
        if error.is_timeout() {
            let during = if error.is_connect() {
                PublicationTimeoutContext::Connect
            } else if error.is_decode() || error.is_body() {
                PublicationTimeoutContext::Body
            } else {
                PublicationTimeoutContext::Request
            };
            Self::Timeout { during }
        } else if let Some(status) = error.status() {
            Self::HttpStatus {
                status: status.as_u16(),
            }
        } else if error.is_connect() {
            Self::Connect
        } else if error.is_decode() {
            Self::Decode
        } else if error.is_body() {
            Self::Body
        } else if error.is_request() {
            Self::Request
        } else if error.is_builder() {
            Self::Builder
        } else if error.is_redirect() {
            Self::Redirect
        } else {
            Self::Other
        }
    }
}

fn safe_io_cause(error: &reqwest::Error) -> Option<PublicationIoFailure> {
    use std::error::Error;
    let mut cause = error.source();
    // Emit only fixed IO classifications and OS numbers, never arbitrary URLs, headers or body text.
    for _ in 0..8 {
        let current = cause?;
        if let Some(error) = current.downcast_ref::<std::io::Error>() {
            return Some(PublicationIoFailure {
                kind: format!("{:?}", error.kind()),
                os_error: error.raw_os_error(),
            });
        }
        cause = current.source();
    }
    None
}

/// Only a reconciled complete import can authorize the subsequent archive step.
pub struct PublishedProject {
    bundle_digest: CanonicalHash,
    import_id: RequestId,
    head: ReplicationPosition,
}
impl PublishedProject {
    pub fn import_id(&self) -> RequestId {
        self.import_id
    }
    pub fn head(&self) -> ReplicationPosition {
        self.head
    }
    pub fn matches_bundle(
        &self,
        bundle: &PreparedProjectBundle,
    ) -> Result<bool, ProjectBundleError> {
        Ok(self.bundle_digest
            == crate::bundle_files::bytes_digest(&serde_json::to_vec(bundle.manifest())?)?)
    }
}

pub struct StagingPublisher {
    endpoint: Url,
    client: reqwest::Client,
}

impl StagingPublisher {
    /// Debug-only read of an existing sealed import; never starts or advances publication.
    #[cfg(debug_assertions)]
    pub async fn inspect_import_status(
        &self,
        bundle: &PreparedProjectBundle,
    ) -> Result<Option<AdminImportRecord>, PublicationError> {
        if bundle.manifest().target_origin != self.endpoint.origin().ascii_serialization() {
            return Err(PublicationError::TargetMismatch);
        }
        require_unchanged_bundle(bundle)?;
        self.status(bundle).await
    }

    pub fn from_owner_file(endpoint: Url, path: &Path) -> Result<Self, PublicationError> {
        validate_endpoint(&endpoint)?;
        let headers = headers_from_private_file(&endpoint, path, &SystemClock)?;
        let client = build_owner_client(headers)?;
        Ok(Self { endpoint, client })
    }

    pub async fn from_owner_access(endpoint: Url) -> Result<Self, PublicationError> {
        validate_endpoint(&endpoint)?;
        let client = owner_access_client(&endpoint).await?;
        Ok(Self { endpoint, client })
    }

    pub async fn publish(
        &self,
        host: &mut OfflineHost,
        bundle: &PreparedProjectBundle,
    ) -> Result<PublishedProject, PublicationError> {
        if bundle.manifest().target_origin != self.endpoint.origin().ascii_serialization() {
            return Err(PublicationError::TargetMismatch);
        }
        let selected = &bundle.manifest().source.selected;
        if selected.peer != host.peer()
            || bundle.manifest().source_directory
                != host
                    .collaboration_directory()
                    .join("projects")
                    .join(selected.project.as_str())
            || bundle.manifest().backup_directory
                != crate::bundle_files::backup_directory(
                    host.router_root(),
                    bundle.manifest().run_id,
                    &selected.project,
                )?
        {
            return Err(PublicationError::SourceHostMismatch);
        }
        // Reopen validates every sealed page before the first external effect.
        require_unchanged_bundle(bundle)?;
        // A valid seal cannot authorize replay after local content advances.
        // Check before Fleet import as well as the project publication requests.
        require_unchanged_source_or_backup(bundle)?;
        if bundle
            .manifest()
            .source_directory
            .try_exists()
            .map_err(ProjectBundleError::Io)?
        {
            // Pre-fix seals could fingerprint a newer sibling without exporting
            // it. Recheck the same native admission used by fresh preparation.
            drop(crate::native_project_snapshot::admit_selected_source(host, selected).await?);
        }
        let fleet: FleetImportResult = self
            .post(
                "/admin/v1/import/fleet",
                &bundle.manifest().fleet,
                PublicationStage::Fleet,
            )
            .await?;
        match fleet {
            FleetImportResult::Imported { .. } | FleetImportResult::AlreadyImported => {}
            FleetImportResult::Refused { .. } => {
                return Err(PublicationError::FleetRefused {
                    answer: Box::new(fleet),
                });
            }
        }
        let previous = self.status(bundle).await?;
        let header = bundle.manifest().header.clone();
        // Begin also checks current retirement/reservation state for completed runs.
        let begin = self
            .project(
                bundle,
                &AdminProjectImportRequest::Begin { header },
                PublicationStage::Begin,
            )
            .await?;
        let import_id = match begin {
            AdminProjectImportAnswer::Ready { import_id, .. } => {
                if previous
                    .as_ref()
                    .is_some_and(|record| record.import_id != import_id)
                {
                    return Err(PublicationError::StatusMismatch);
                }
                import_id
            }
            AdminProjectImportAnswer::AlreadyImported => {
                return self
                    .confirmed(bundle, previous.as_ref().map(|record| record.import_id))
                    .await;
            }
            AdminProjectImportAnswer::Imported { head }
                if head == bundle.manifest().header.manifest.position =>
            {
                return self
                    .confirmed(bundle, previous.as_ref().map(|record| record.import_id))
                    .await;
            }
            answer => {
                return Err(PublicationError::Incomplete {
                    answer: Box::new(answer),
                });
            }
        };
        for index in 0..bundle.manifest().header.pages {
            let contents = bundle.page(index)?;
            // Refuse any decode/re-encode drift; the frozen bytes own the payload.
            if serde_json::to_vec(&contents)? != bundle.page_bytes(index)? {
                return Err(ProjectBundleError::InvalidArtifact.into());
            }
            let page_digest = bundle
                .manifest()
                .page_digests
                .get(usize::try_from(index).map_err(|_| ProjectBundleError::InvalidArtifact)?)
                .ok_or(ProjectBundleError::InvalidArtifact)?
                .clone();
            let request = AdminProjectImportRequest::Page {
                digest: bundle.manifest().header.digest.clone(),
                import_id,
                page: index,
                contents,
                page_digest,
            };
            let answer = self
                .project(bundle, &request, PublicationStage::Page { index })
                .await?;
            if answer
                != (AdminProjectImportAnswer::Staged {
                    import_id,
                    page: index,
                })
            {
                return Err(PublicationError::Incomplete {
                    answer: Box::new(answer),
                });
            }
        }
        let answer = self
            .project(
                bundle,
                &AdminProjectImportRequest::Complete {
                    digest: bundle.manifest().header.digest.clone(),
                    import_id,
                },
                PublicationStage::Complete,
            )
            .await?;
        match answer {
            AdminProjectImportAnswer::Imported { head }
                if head == bundle.manifest().header.manifest.position => {}
            AdminProjectImportAnswer::AlreadyImported => {}
            answer => {
                return Err(PublicationError::Incomplete {
                    answer: Box::new(answer),
                });
            }
        }
        self.confirmed(bundle, Some(import_id)).await
    }

    async fn confirmed(
        &self,
        bundle: &PreparedProjectBundle,
        expected_id: Option<RequestId>,
    ) -> Result<PublishedProject, PublicationError> {
        let record = self.completed_record(bundle, expected_id).await?;
        Ok(PublishedProject {
            bundle_digest: crate::bundle_files::bytes_digest(&serde_json::to_vec(
                bundle.manifest(),
            )?)?,
            import_id: record.import_id,
            head: record.header.manifest.position,
        })
    }

    /// Observe completion before Claim; this grants no authority to archive a source.
    pub(crate) async fn require_complete_import(
        &self,
        bundle: &PreparedProjectBundle,
    ) -> Result<(), PublicationError> {
        if bundle.manifest().target_origin != self.endpoint.origin().ascii_serialization() {
            return Err(PublicationError::TargetMismatch);
        }
        require_unchanged_bundle(bundle)?;
        self.completed_record(bundle, None).await?;
        Ok(())
    }

    async fn completed_record(
        &self,
        bundle: &PreparedProjectBundle,
        expected_id: Option<RequestId>,
    ) -> Result<AdminImportRecord, PublicationError> {
        let record = self
            .status(bundle)
            .await?
            .ok_or(PublicationError::StatusMismatch)?;
        if expected_id.is_some_and(|id| record.import_id != id)
            || record.phase
                != (AdminImportPhase::Complete {
                    position: bundle.manifest().header.manifest.position,
                })
        {
            return Err(PublicationError::StatusMismatch);
        }
        Ok(record)
    }

    async fn status(
        &self,
        bundle: &PreparedProjectBundle,
    ) -> Result<Option<AdminImportRecord>, PublicationError> {
        let answer = self
            .project(
                bundle,
                &AdminProjectImportRequest::Status {
                    digest: bundle.manifest().header.digest.clone(),
                },
                PublicationStage::Status,
            )
            .await?;
        match answer {
            AdminProjectImportAnswer::Progress(record) => {
                if record.header != bundle.manifest().header {
                    return Err(PublicationError::StatusMismatch);
                }
                match record.phase {
                    AdminImportPhase::Reserved => {}
                    AdminImportPhase::Published { position }
                    | AdminImportPhase::Complete { position }
                        if position == record.header.manifest.position => {}
                    AdminImportPhase::Published { .. } | AdminImportPhase::Complete { .. } => {
                        return Err(PublicationError::StatusMismatch);
                    }
                }
                Ok(Some(record))
            }
            AdminProjectImportAnswer::Refused(AdminImportRefusal::NotFound) => Ok(None),
            answer => Err(PublicationError::Incomplete {
                answer: Box::new(answer),
            }),
        }
    }

    async fn project(
        &self,
        bundle: &PreparedProjectBundle,
        request: &AdminProjectImportRequest,
        stage: PublicationStage,
    ) -> Result<AdminProjectImportAnswer, PublicationError> {
        self.post(
            &format!(
                "/admin/v1/import/projects/{}",
                bundle.manifest().source.selected.project.as_str()
            ),
            request,
            stage,
        )
        .await
    }

    async fn post<Input: Serialize + ?Sized, Output: DeserializeOwned>(
        &self,
        path: &str,
        input: &Input,
        stage: PublicationStage,
    ) -> Result<Output, PublicationError> {
        let mut endpoint = self.endpoint.clone();
        endpoint.set_path(path);
        let response = self
            .client
            .post(endpoint)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(serde_json::to_vec(input)?)
            .send()
            .await
            .map_err(|source| PublicationError::OutcomeUnknown { stage, source })?;
        if matches!(
            response.status(),
            reqwest::StatusCode::GONE | reqwest::StatusCode::SERVICE_UNAVAILABLE
        ) {
            let failure = response
                .json()
                .await
                .map_err(|source| PublicationError::OutcomeUnknown { stage, source })?;
            return Err(PublicationError::OwnerFailure {
                failure: Box::new(failure),
            });
        }
        let response = response
            .error_for_status()
            .map_err(|source| PublicationError::OutcomeUnknown { stage, source })?;
        response
            .json()
            .await
            .map_err(|source| PublicationError::OutcomeUnknown { stage, source })
    }
}

fn require_unchanged_source_or_backup(
    bundle: &PreparedProjectBundle,
) -> Result<(), PublicationError> {
    let manifest = bundle.manifest();
    let source = manifest
        .source_directory
        .try_exists()
        .map_err(ProjectBundleError::Io)?;
    let backup = manifest
        .backup_directory
        .try_exists()
        .map_err(ProjectBundleError::Io)?;
    let unchanged = match (source, backup) {
        (true, false) => manifest
            .source_fingerprint
            .matches(&manifest.source_directory)?,
        // A lost local result may resume after the exact original directory was moved.
        (false, true) => manifest
            .source_fingerprint
            .matches(&manifest.backup_directory)?,
        (true, true) | (false, false) => false,
    };
    if !unchanged {
        return Err(PublicationError::SourceChanged);
    }
    Ok(())
}

fn require_unchanged_bundle(bundle: &PreparedProjectBundle) -> Result<(), PublicationError> {
    let checked = PreparedProjectBundle::open(bundle.directory())?;
    if serde_json::to_vec(checked.manifest())? != serde_json::to_vec(bundle.manifest())? {
        return Err(PublicationError::StatusMismatch);
    }
    Ok(())
}

fn validate_endpoint(endpoint: &Url) -> Result<(), PublicationError> {
    if !matches!(endpoint.scheme(), "https" | "http")
        || endpoint.host_str().is_none()
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
        || endpoint.path() != "/"
    {
        return Err(PublicationError::TargetMismatch);
    }
    Ok(())
}
