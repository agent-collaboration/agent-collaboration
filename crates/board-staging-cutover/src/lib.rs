//! One-time staging cutover, separate from the running Router and its stores.
mod bundle_files;
mod source_archive;
pub use source_archive::{SourceArchiveError, SourceArchiveOutcome, archive_confirmed_source};
mod offline_host;
mod owner_publication_access;
mod prepared_project_bundle;
mod project_bundle_error;
pub use project_bundle_error::ProjectBundleError;
mod snapshot_transport;
mod staging_publication;
pub use bundle_files::SourceDirectoryFingerprint;
pub use owner_publication_access::OwnerPublicationAccessError;
pub use prepared_project_bundle::{
    PreparedProjectBundle, ProjectBundleManifest, ProjectPreparation,
};
pub use staging_publication::{
    PublicationError, PublicationStage, PublishedProject, StagingPublisher,
};

pub use offline_host::{OfflineHost, OfflineHostError};

pub use snapshot_transport::{
    PreparedSnapshotPage, SnapshotTransportError, reframe_import_manifest, reframe_import_page,
};

mod native_project_snapshot;
pub use native_project_snapshot::{
    NativeProjectSnapshot, NativeSourceError, NativeSourcePreflight, SelectedWriterFile,
};

#[cfg(test)]
mod cutover_host_fixture;

#[cfg(test)]
mod native_snapshot_fixture;
