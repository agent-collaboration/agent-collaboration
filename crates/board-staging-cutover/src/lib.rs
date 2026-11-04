//! One-time staging cutover, separate from the running Router and its stores.
mod offline_host;
mod snapshot_transport;

pub use offline_host::{OfflineHost, OfflineHostError};

pub use snapshot_transport::{
    PreparedSnapshotPage, SnapshotTransportError, reframe_import_manifest, reframe_import_page,
};
