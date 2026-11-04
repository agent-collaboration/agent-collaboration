//! One-time staging cutover, separate from the running Router and its stores.
mod bundle_files;
mod offline_host;
mod prepared_project_bundle;
mod project_bundle_error;
pub use project_bundle_error::ProjectBundleError;
mod snapshot_transport;
pub use bundle_files::SourceDirectoryFingerprint;
pub use prepared_project_bundle::{
    PreparedProjectBundle, ProjectBundleManifest, ProjectPreparation,
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
