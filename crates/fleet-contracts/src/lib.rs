//! Shared vocabulary and interfaces for FleetRuntime features.
#![forbid(unsafe_code)]

mod central_observation;
mod client_id;
mod event_completion;
mod issuer_refusals;
mod machine_connection;
mod machine_connection_refusal;
mod provisioning_failure;
mod statement_refusal;

pub use central_observation::CentralObservation;
pub use client_id::ClientId;
pub use event_completion::{AlarmNotPublished, AlarmResult, EventCompletion, EventCompletionHook};
pub use issuer_refusals::{IssuerKeyRefusal, IssuerKeyResolutionRefusal, IssuerSignatureRefusal};
pub use machine_connection::{LiveMachineConnection, MachineConnectionAttachment};
pub use machine_connection_refusal::MachineConnectionRefusal;
pub use provisioning_failure::OrganizationProvisioningFailure;
pub use statement_refusal::FleetOrganizationStatementRefusal;
