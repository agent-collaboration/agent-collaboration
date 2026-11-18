//! Types and traits shared by FleetRuntime and its feature crates, so features
//! meet here instead of depending on each other.
#![forbid(unsafe_code)]

mod central_observation;
mod event_completion;
mod issuer_refusals;
mod statement_refusal;
mod workspace_caller;

pub use central_observation::CentralObservation;
pub use event_completion::{AlarmNotPublished, AlarmResult, EventCompletion, EventCompletionHook};
pub use issuer_refusals::{IssuerKeyRefusal, IssuerKeyResolutionRefusal, IssuerSignatureRefusal};
pub use statement_refusal::FleetOrganizationStatementRefusal;
pub use workspace_caller::WorkspaceCaller;
