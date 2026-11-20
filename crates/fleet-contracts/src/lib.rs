//! Shared vocabulary and interfaces for FleetRuntime features.
#![forbid(unsafe_code)]

mod central_observation;
mod event_completion;
mod issuer_refusals;
mod provisioning_failure;
mod statement_refusal;

pub use central_observation::CentralObservation;
pub use event_completion::{AlarmNotPublished, AlarmResult, EventCompletion, EventCompletionHook};
pub use issuer_refusals::{IssuerKeyRefusal, IssuerKeyResolutionRefusal, IssuerSignatureRefusal};
pub use provisioning_failure::OrganizationProvisioningFailure;
pub use statement_refusal::FleetOrganizationStatementRefusal;
