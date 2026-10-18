//! Per-project native Turso stores and the Router's fleet/local storage.
#![cfg_attr(test, allow(clippy::panic, clippy::panic_in_result_fn))]
mod cursor_store;
#[cfg(test)]
mod fleet_archive_report_tests;
mod fleet_archive_reports;
mod fleet_cache;
mod fleet_decision_writes;
mod fleet_outbox;
mod fleet_record_codec;
mod fleet_snapshot_reads;
mod fleet_storage_error;
mod fleet_store;
mod page_cursors;
pub use page_cursors::{decode_board_cursor, encode_board_cursor};
mod project_assignment_reads;
mod project_bookkeeping;
mod project_card_operands;
mod project_context_reads;
mod project_epoch_reads;
mod project_feed_pages;
mod project_inbox_pages;
mod project_intent_io;
mod project_ledger_reader;
mod project_membership_io;
mod project_participant_batch;
mod project_participant_io;
mod project_participant_pages;
mod project_post_actor_reads;
mod project_process_operands;
mod project_reader_mark_io;
mod project_reader_metadata;
mod project_record_batch;
mod project_record_io;
mod project_record_reads;
mod project_record_relations;
mod project_recovery_reads;
mod project_request_void;
mod project_role_operands;
mod project_schema;
mod project_storage_error;
mod project_store;
mod project_subscription_reads;
mod project_subtask_reads;
mod project_task_operands;
mod project_thread_pages;
mod project_trail_pages;
mod project_view_reads;
mod project_view_trail_metadata;
mod project_watch_io;
pub use project_ledger_reader::ProjectLedgerReader;
#[cfg(test)]
mod project_rejection_tests;
#[cfg(test)]
mod project_void_probe;
mod router_local_database;
pub mod schema_fingerprint;

pub use cursor_store::{
    CursorSigningKey, CursorStore, LegacyCursorScope, LegacyPosition, LegacyScopeTranslation,
};
pub use fleet_cache::{
    FleetCache, FleetCacheSnapshot, FleetDiscoveryKey, FleetListFilter, StoredFleetRecord,
};
pub use fleet_outbox::{FleetOutbox, FleetOutboxEntry, FleetOutboxState};
pub use fleet_storage_error::FleetStorageError;
pub use fleet_store::{FLEET_MIGRATOR, FleetFeedPage, FleetRecordSnapshot, FleetReply, FleetStore};
pub use project_intent_io::IntentPageKey;
pub use project_participant_batch::ParticipantBatch;
pub use project_participant_pages::{ParticipantIntervalBatch, ParticipantIntervalKey};
pub use project_record_batch::StoredProjectRecord;
pub use project_record_reads::LinkMutation;
pub use project_schema::PROJECT_MIGRATOR;
pub use project_schema::StoreSchemaState;
pub use project_storage_error::ProjectStorageError;
pub use project_store::{
    ProjectCommitGuard, ProjectSnapshot, ProjectStore, ProjectWriteTransaction, ProjectWriterGuard,
};
pub use project_subscription_reads::PostActivityKey;
pub use router_local_database::RouterLocalStorageError;

#[cfg(test)]
mod fleet_store_tests;
#[cfg(test)]
mod owner_grant_tests;
#[cfg(test)]
mod project_crash_tests;
#[cfg(test)]
mod project_intent_tests;
#[cfg(test)]
mod project_membership_tests;
#[cfg(test)]
mod project_migration_policy;
#[cfg(test)]
mod project_move_tests;
#[cfg(test)]
mod project_participant_tests;
#[cfg(test)]
mod project_relation_tests;
#[cfg(test)]
mod project_schema_tests;
#[cfg(test)]
mod project_store_tests;
#[cfg(test)]
mod project_subscription_tests;
#[cfg(test)]
mod project_upsert_cdc_tests;
#[cfg(test)]
mod router_local_store_tests;
#[cfg(test)]
mod schema_fingerprint_tests;

pub use project_feed_pages::{
    FeedPageBounds, FeedPageDirection, FeedPageKey, PostPageFilter, PostPageScope, StoredFeedEntry,
};

pub use project_inbox_pages::ReaderTrackingMetadata;

pub use project_view_trail_metadata::{
    HistoricalLeadMetadata, PendingTrailInputs, PendingTrailSelection, TrailActivityMetadata,
    TrailDecisionMetadata,
};

pub use project_card_operands::CardWorkOperands;

pub use project_trail_pages::TrailPageFilter;
