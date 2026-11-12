//! One Turso transaction owner for typed interactions: approvals, questions and their routes.
mod interaction_store;
pub use interaction_store::InteractionStore;
mod error;
pub use error::StorageError;
mod approval_routes;
pub use approval_routes::StoredApprovalRoute;
/// Retention shared by settled interaction records in `interaction.sqlite`.
pub const INTERACTION_RETENTION_MONTHS: u32 = 2;
mod interaction_history;
pub use interaction_history::{
    InteractionHistoryError, InteractionHistoryRecord, InteractionHistoryState,
    InteractionHistoryStore, LegacyApprovalMetadata, QuestionHistoryState, QuestionResponse,
    RefusedApprovalOption, RefusedTypedApproval,
};
mod interaction_retention;
pub use interaction_retention::MAX_INTERACTION_PRUNE_BATCH;

#[cfg(test)]
#[path = "composed_schema_upgrade_test.rs"]
mod composed_schema_upgrade_tests;
#[cfg(test)]
#[path = "library_journal_upgrade_test.rs"]
mod library_journal_upgrade_tests;
#[cfg(test)]
mod schema_prefix_fixture;
