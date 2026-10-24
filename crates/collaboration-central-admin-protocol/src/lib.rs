//! Runtime-neutral wire contract for central's owner-only `/admin/v1/*` API.
mod copy_health;
mod copy_operations;
mod registry_operations;
pub use copy_health::*;
pub use copy_operations::*;
pub use fleet_sync_protocol::{
    DatabaseId, Epoch, HolderChangeReason, ReplicationPosition, RequestId, TakeoverPreview,
    TransferId,
};
pub use registry_operations::*;
