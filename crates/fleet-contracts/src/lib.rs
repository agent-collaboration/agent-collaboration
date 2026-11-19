//! Types and traits shared by FleetRuntime and its feature crates, so features
//! meet here instead of depending on each other.
#![forbid(unsafe_code)]

mod central_observation;
mod event_completion;
mod issuer_refusals;
mod statement_refusal;
mod workspace_caller;
pub mod workspace_description;
mod workspace_directory;
mod workspace_drain;

pub use central_observation::CentralObservation;
pub use event_completion::{AlarmNotPublished, AlarmResult, EventCompletion, EventCompletionHook};
pub use issuer_refusals::{IssuerKeyRefusal, IssuerKeyResolutionRefusal, IssuerSignatureRefusal};
pub use statement_refusal::FleetOrganizationStatementRefusal;
pub use workspace_caller::WorkspaceCaller;
pub use workspace_description::MAXIMUM_DESCRIPTION_BYTES;
pub use workspace_directory::{DirectoryWorkspace, WorkspaceDirectory, WorkspaceView};
pub use workspace_drain::WorkspaceDrain;
