//! Tokio capabilities for the runtime-independent central core.
#![forbid(unsafe_code)]

mod native_logging;
mod object_storage;
#[cfg(unix)]
mod organization_signin_link;
#[cfg(unix)]
pub use organization_signin_link::{OrganizationSignInLink, OrganizationSignInLinkError};
mod sign_in_crypto;
mod sign_in_trust;
#[cfg(feature = "test-support")]
pub use object_storage::ObjectStorage;
pub use object_storage::StoreMigrations;
pub use sign_in_trust::NativeSignInTrust;
mod workspace_host;
pub use workspace_host::{TokioWorkspaceHost, TokioWorkspaceStores};
mod machine_push_registry;
mod workspace_link;
pub use workspace_link::WorkspaceLink;
#[cfg(unix)]
mod workspace_server;
#[cfg(unix)]
pub use workspace_server::{TokioWorkspaceServer, TokioWorkspaceServerError};
