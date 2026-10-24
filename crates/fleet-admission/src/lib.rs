//! Shared fleet decisions: no storage, runtime, clock or random-ID generation.
#![forbid(unsafe_code)]
mod archive_report;
mod directory_query;
mod fleet_adr_rules;
mod fleet_decision;
mod fleet_import_contract;
mod fleet_import_rules;
pub use archive_report::{
    ArchiveFinalization, ArchiveReport, ArchiveReportDecision, ArchiveReportOutcome,
    ArchiveReportRefusal, decide_archive_finalization, decide_archive_report,
};
pub use directory_query::{
    CopyMetadata, CopyMetadataAsOf, DirectoryEntry, DirectoryLease, DirectoryLeaseState,
    DirectoryQuery, DirectorySide, LeaseMetadata,
};
mod fleet_metadata_rules;
mod fleet_reads;
mod fleet_request;
mod fleet_rules;
pub use fleet_import_contract::*;
pub use fleet_import_rules::{decide_import, reads_for_import};

pub use fleet_decision::{
    FleetAdmissionError, FleetDecision, FleetFeedEntry, FleetIds, FleetRequestResult, FleetWrite,
};
pub use fleet_reads::{
    FleetNameOwner, FleetNameScope, FleetReadKey, FleetReads, FleetSnapshot,
    ProjectCreationDecision, reads_for,
};
pub use fleet_request::{FleetOperation, FleetRequest, ProcessDefinitionInput, canonical_hash};
pub use fleet_rules::decide;

#[cfg(feature = "test-support")]
pub mod conformance_cases;

#[cfg(test)]
mod archive_report_tests;
#[cfg(test)]
mod fleet_creation_tests;
#[cfg(test)]
mod fleet_import_tests;
#[cfg(test)]
mod fleet_rule_tests;
