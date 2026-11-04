//! Freeze unchanged native rows and request identities before publication.
use crate::ProjectBundleError;
use crate::bundle_files::{
    SourceDirectoryFingerprint, bytes_digest, commit_directory, owned_directory, read_small,
    write_frozen,
};
use crate::{
    NativeProjectSnapshot, NativeSourcePreflight, OfflineHost, SelectedWriterFile,
    reframe_import_manifest,
};
use board_sync_protocol::BoardAnnotation;
use collaboration_central_core::AdminImportHeader;
use fleet_admission::{FleetImportBundle, FleetImportRecord, FleetImportResult, FleetSnapshot};
use fleet_sync_protocol::{ImportDigest, PageDigest, SnapshotManifest, SnapshotPage};
use message_board::{CanonicalHash, RequestId};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    path::{Path, PathBuf},
};
use url::Url;
use uuid::Uuid;

#[derive(Clone)]
pub struct ProjectPreparation {
    pub run_id: Uuid,
    pub target: Url,
    pub fleet: FleetImportBundle,
    pub claim_request: RequestId,
    pub canary_request: RequestId,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectBundleManifest {
    pub run_id: Uuid,
    pub target_origin: String,
    pub fleet: FleetImportBundle,
    pub claim_request: RequestId,
    pub canary_request: RequestId,
    pub source: NativeSourcePreflight,
    pub source_directory: PathBuf,
    pub source_fingerprint: SourceDirectoryFingerprint,
    pub backup_directory: PathBuf,
    pub header: AdminImportHeader,
    pub page_digests: Vec<PageDigest>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BundleSeal {
    manifest_digest: CanonicalHash,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ImportContent<'a> {
    format: &'static str,
    manifest: &'a SnapshotManifest,
    pages: &'a [PageDigest],
}

pub struct PreparedProjectBundle {
    directory: PathBuf,
    manifest: ProjectBundleManifest,
}

impl PreparedProjectBundle {
    pub async fn prepare(
        host: &mut OfflineHost,
        selected: SelectedWriterFile,
        input: ProjectPreparation,
        parent: &Path,
    ) -> Result<Self, ProjectBundleError> {
        Self::prepare_with_space_probe(host, selected, input, parent, |path| {
            fs2::available_space(path)
        })
        .await
    }

    async fn prepare_with_space_probe(
        host: &mut OfflineHost,
        selected: SelectedWriterFile,
        input: ProjectPreparation,
        parent: &Path,
        available_space: impl Fn(&Path) -> std::io::Result<u64>,
    ) -> Result<Self, ProjectBundleError> {
        validate_input(&input)?;
        validate_graph(&input.fleet, &selected)?;
        if !parent.is_absolute() {
            return Err(ProjectBundleError::UnsafePath);
        }
        owned_directory(parent, true)?;
        let source_path = crate::native_project_snapshot::selected_path(host, &selected)?;
        let source_directory = source_path
            .parent()
            .ok_or(ProjectBundleError::UnsafePath)?
            .to_owned();
        // Artifacts must not become part of the directory that will be archived.
        let parent = std::fs::canonicalize(parent)?;
        if parent.starts_with(&source_directory) {
            return Err(ProjectBundleError::UnsafePath);
        }
        let final_name = format!("{}-{}", input.run_id, selected.project.as_str());
        if std::fs::symlink_metadata(parent.join(&final_name)).is_ok() {
            return Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists).into());
        }
        let mut source = NativeProjectSnapshot::open(host, selected).await?;
        let preflight = source.preflight().clone();
        let exported = async {
            validate_scope(&input.fleet, &preflight)?;
            require_bundle_capacity(&mut source, &parent, available_space).await?;
            let partial = tempfile::Builder::new()
                .prefix(".cutover-partial-")
                .permissions(std::os::unix::fs::PermissionsExt::from_mode(0o700))
                .tempdir_in(&parent)?;
            let pages_path = partial.path().join("pages");
            std::fs::create_dir(&pages_path)?;
            std::fs::set_permissions(
                &pages_path,
                std::os::unix::fs::PermissionsExt::from_mode(0o700),
            )?;
            let mut digests = Vec::new();
            while let Some(page) = source.next_page().await? {
                let index = u32::try_from(digests.len())
                    .map_err(|_| ProjectBundleError::InvalidArtifact)?;
                write_frozen(
                    &pages_path.join(page_name(index)),
                    &serde_json::to_vec(&page.contents)?,
                )?;
                digests.push(page.digest);
            }
            Ok::<_, ProjectBundleError>((digests, partial))
        }
        .await;
        let closed = source.close().await;
        let (page_digests, partial) = exported?;
        closed?;
        let manifest = reframe_import_manifest(preflight.manifest.clone())?;
        let digest = import_digest(&manifest, &page_digests)?;
        let pages =
            u32::try_from(page_digests.len()).map_err(|_| ProjectBundleError::InvalidArtifact)?;
        if pages == 0 {
            return Err(ProjectBundleError::InvalidArtifact);
        }
        let header = AdminImportHeader {
            digest,
            provenance: input.fleet.provenance.clone(),
            declared_owner: input.fleet.declared_owner.clone(),
            manifest,
            pages,
            summary: BoardAnnotation::ProjectSummary {
                latest_record_at: preflight.latest_record_at,
            },
        };
        let backup_directory = crate::bundle_files::backup_directory(
            host.router_root(),
            input.run_id,
            &preflight.selected.project,
        )?;
        let metadata = ProjectBundleManifest {
            run_id: input.run_id,
            target_origin: input.target.origin().ascii_serialization(),
            fleet: input.fleet,
            claim_request: input.claim_request,
            canary_request: input.canary_request,
            source_fingerprint: SourceDirectoryFingerprint::capture(&source_directory)?,
            source: preflight,
            source_directory,
            backup_directory,
            header,
            page_digests,
        };
        let bytes = serde_json::to_vec(&metadata)?;
        if u64::try_from(bytes.len()).map_err(|_| ProjectBundleError::InvalidArtifact)?
            > crate::bundle_files::MAX_INDEX_BYTES
        {
            return Err(ProjectBundleError::InvalidArtifact);
        }
        write_frozen(&partial.path().join("manifest.json"), &bytes)?;
        write_frozen(
            &partial.path().join("seal.json"),
            &serde_json::to_vec(&BundleSeal {
                manifest_digest: bytes_digest(&bytes)?,
            })?,
        )?;
        File::open(partial.path().join("pages"))?.sync_all()?;
        let directory = commit_directory(&parent, partial.path(), &final_name)?;
        // The temporary path was moved atomically; dropping it has nothing to remove.
        drop(partial);
        Self::open(&directory)
    }

    pub fn open(directory: &Path) -> Result<Self, ProjectBundleError> {
        owned_directory(directory, true)?;
        owned_directory(&directory.join("pages"), true)?;
        let (seal, _): (BundleSeal, _) = read_small(&directory.join("seal.json"))?;
        let (manifest, bytes): (ProjectBundleManifest, _) =
            read_small(&directory.join("manifest.json"))?;
        if bytes_digest(&bytes)? != seal.manifest_digest {
            return Err(ProjectBundleError::InvalidArtifact);
        }
        validate_manifest(&manifest)?;
        let bundle = Self {
            directory: directory.to_owned(),
            manifest,
        };
        for page in 0..bundle.manifest.header.pages {
            bundle.page(page)?;
        }
        Ok(bundle)
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }
    pub fn manifest(&self) -> &ProjectBundleManifest {
        &self.manifest
    }

    pub fn page(&self, index: u32) -> Result<SnapshotPage, ProjectBundleError> {
        self.read_page(index).map(|(page, _)| page)
    }

    pub fn page_bytes(&self, index: u32) -> Result<Vec<u8>, ProjectBundleError> {
        self.read_page(index).map(|(_, bytes)| bytes)
    }

    fn read_page(&self, index: u32) -> Result<(SnapshotPage, Vec<u8>), ProjectBundleError> {
        let expected = self
            .manifest
            .page_digests
            .get(usize::try_from(index).map_err(|_| ProjectBundleError::InvalidArtifact)?)
            .ok_or(ProjectBundleError::InvalidArtifact)?;
        let (page, bytes): (SnapshotPage, _) =
            read_small(&self.directory.join("pages").join(page_name(index)))?;
        let target = &self.manifest.header.manifest;
        if bytes_digest(&bytes)?.as_str() != expected.as_str()
            || PageDigest::of(&page)? != *expected
            || page.database != target.database
            || page.position != target.position
            || page.schema != target.schema
            || page
                .next
                .as_ref()
                .is_some_and(|next| next.position != target.position)
            || (page.next.is_none() != (index.checked_add(1) == Some(self.manifest.header.pages)))
        {
            return Err(ProjectBundleError::InvalidArtifact);
        }
        Ok((page, bytes))
    }
}

async fn require_bundle_capacity(
    source: &mut NativeProjectSnapshot<'_>,
    parent: &Path,
    available_space: impl Fn(&Path) -> std::io::Result<u64>,
) -> Result<(), ProjectBundleError> {
    // Count the exact encoded pages with bounded memory, then replay the same
    // pinned snapshot. Reserve the existing format bounds for manifest + seal.
    // This is a preflight, not a filesystem reservation; later I/O can still fail.
    let mut required = crate::bundle_files::MAX_INDEX_BYTES * 2;
    while let Some(page) = source.next_page().await? {
        let bytes = u64::try_from(serde_json::to_vec(&page.contents)?.len())
            .map_err(|_| ProjectBundleError::InvalidArtifact)?;
        required = required
            .checked_add(bytes)
            .ok_or(ProjectBundleError::InvalidArtifact)?;
    }
    let available = available_space(parent)?;
    if available < required {
        return Err(ProjectBundleError::InsufficientSpace {
            available,
            required,
        });
    }
    source.rewind_pages();
    Ok(())
}

fn import_digest(
    manifest: &SnapshotManifest,
    pages: &[PageDigest],
) -> Result<ImportDigest, ProjectBundleError> {
    Ok(ImportDigest::of_canonical(&ImportContent {
        format: "native-staging-cutover-v1",
        manifest,
        pages,
    })?)
}

fn page_name(index: u32) -> String {
    format!("{index:010}.json")
}

pub(crate) fn validate_input(input: &ProjectPreparation) -> Result<(), ProjectBundleError> {
    if input.run_id.get_version_num() != 7
        || input.target.host_str().is_none()
        || !matches!(input.target.scheme(), "https" | "http")
        || !input.target.username().is_empty()
        || input.target.password().is_some()
        || input.target.query().is_some()
        || input.target.fragment().is_some()
        || input.target.path() != "/"
        || input.claim_request == input.canary_request
    {
        return Err(ProjectBundleError::InvalidPreparation);
    }
    Ok(())
}

fn validate_graph(
    fleet: &FleetImportBundle,
    selected: &SelectedWriterFile,
) -> Result<(), ProjectBundleError> {
    let decision = fleet_admission::decide_import(fleet, &FleetSnapshot::empty())
        .map_err(|_| ProjectBundleError::InvalidGraph)?;
    if !matches!(decision.result, FleetImportResult::Imported { .. })
        || !fleet.records.iter().any(|record| matches!(record, FleetImportRecord::Directory { project, .. } if project == &selected.project))
    { return Err(ProjectBundleError::InvalidGraph); }
    Ok(())
}

fn validate_scope(
    fleet: &FleetImportBundle,
    source: &NativeSourcePreflight,
) -> Result<(), ProjectBundleError> {
    let Some(board) = fleet.records.iter().find_map(|record| match record {
        FleetImportRecord::Directory { project, board, .. }
            if project == &source.selected.project =>
        {
            Some(board)
        }
        _ => None,
    }) else {
        return Err(ProjectBundleError::InvalidGraph);
    };
    if source.project_scope.project.as_ref() != Some(&source.selected.project)
        || source.project_scope.board.as_ref() != Some(board)
        || !fleet.records.iter().any(|record| matches!(record, FleetImportRecord::Board { id, workspace, .. } if id == board && workspace == &source.project_scope.workspace))
    { return Err(ProjectBundleError::InvalidGraph); }
    Ok(())
}

fn validate_manifest(manifest: &ProjectBundleManifest) -> Result<(), ProjectBundleError> {
    let input = ProjectPreparation {
        run_id: manifest.run_id,
        target: Url::parse(&manifest.target_origin)
            .map_err(|_| ProjectBundleError::InvalidPreparation)?,
        fleet: manifest.fleet.clone(),
        claim_request: manifest.claim_request.clone(),
        canary_request: manifest.canary_request.clone(),
    };
    validate_input(&input)?;
    validate_graph(&manifest.fleet, &manifest.source.selected)?;
    validate_scope(&manifest.fleet, &manifest.source)?;
    if manifest.header.manifest != reframe_import_manifest(manifest.source.manifest.clone())?
        || manifest.header.digest
            != import_digest(&manifest.header.manifest, &manifest.page_digests)?
        || usize::try_from(manifest.header.pages).ok() != Some(manifest.page_digests.len())
        || manifest.header.pages == 0
        || manifest.header.declared_owner != manifest.fleet.declared_owner
        || manifest.header.provenance != manifest.fleet.provenance
        || manifest.header.summary
            != (BoardAnnotation::ProjectSummary {
                latest_record_at: manifest.source.latest_record_at,
            })
    {
        return Err(ProjectBundleError::InvalidArtifact);
    }
    Ok(())
}

#[cfg(test)]
#[path = "prepared_project_bundle_test.rs"]
mod tests;
