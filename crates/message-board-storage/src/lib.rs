//! Per-project native Turso stores and the Router's fleet/local storage.
#![cfg_attr(test, allow(clippy::panic, clippy::panic_in_result_fn))]
mod cursor_store;
mod fleet_cache;
mod fleet_outbox;
mod fleet_record_codec;
mod fleet_storage_error;
mod page_cursors;
#[cfg(test)]
mod project_native_integrity_tests;
pub use page_cursors::{decode_board_cursor, encode_board_cursor};
mod project_actor_rows;
mod project_assignment_reads;
mod project_authority_rows;
mod project_authority_writes;
mod project_bookkeeping;
mod project_capture_marks;
mod project_capture_rows;
mod project_card_operands;
mod project_context_reads;
mod project_conversation_rows;
mod project_conversation_writes;
mod project_decision_effects;
mod project_decision_rows;
mod project_decision_writes;
mod project_epoch_reads;
mod project_evidence_rows;
mod project_evidence_writes;
mod project_feed_io;
mod project_feed_pages;
mod project_import_batch;
mod project_import_keys;
pub use project_import_batch::{
    ImportRecord, ImportWatch, ProjectImportBatch, apply_project_import,
};
mod project_inbox_pages;
mod project_inbox_reader_positions;
mod project_intent_io;
mod project_ledger_reader;
#[cfg(test)]
mod project_mail_cancel_expansion_tests;
mod project_mail_cancel_expansions;
#[cfg(test)]
mod project_mail_handoff_tests;
mod project_mail_handoffs;
mod project_membership_io;
mod project_membership_writes;
mod project_participant_batch;
mod project_participant_io;
mod project_participant_pages;
mod project_participation_writes;
mod project_plan_operands;
mod project_post_actor_reads;
mod project_read_store;
#[cfg(test)]
mod project_reader_identity_tests;
mod project_reader_mark_io;
mod project_reader_metadata;
#[cfg(test)]
mod project_receipt_tests;
mod project_record_batch;
mod project_record_codec;
mod project_record_io;
mod project_record_keys;
mod project_record_reads;
mod project_record_relations;
mod project_recovery_reads;
#[cfg(test)]
mod project_request_tests;
mod project_role_operands;
mod project_schema;
mod project_storage_error;
mod project_store;
mod project_subscription_reads;
mod project_subtask_reads;
mod project_task_operands;
#[cfg(test)]
mod project_test_support;
mod project_thread_pages;
#[cfg(test)]
mod project_thread_unread_tests;
mod project_trail_pages;
mod project_trail_writes;
mod project_view_reads;
mod project_view_trail_metadata;
mod project_watch_io;
mod project_work_rows;
mod project_work_writes;
pub use project_import_keys::{ImportRowKeys, ImportRowKind};
pub use project_ledger_reader::ProjectLedgerReader;
mod router_local_database;
pub mod schema_fingerprint;

pub use cursor_store::{CursorSigningKey, CursorStore};
pub use fleet_cache::{
    FleetCache, FleetCacheRebuild, FleetCacheSnapshot, FleetDiscoveryKey, FleetListFilter,
    FleetRecordSnapshot, StoredFleetRecord,
};
pub use fleet_outbox::{FleetOutbox, FleetOutboxEntry, FleetOutboxState};
pub use fleet_storage_error::FleetStorageError;
pub use project_bookkeeping::PROJECT_LOCAL_MIGRATOR;
pub use project_capture_marks::{
    AcknowledgedProjectChanges, record_project_capture, record_snapshot_capture,
};
pub use project_capture_rows::{CapturedFeedSummary, CapturedProjectChanges};
pub use project_epoch_reads::CursorValidity;
pub use project_intent_io::IntentPageKey;
pub use project_mail_cancel_expansions::{MailCancelExpansion, SequencedMailCancelExpansion};
pub use project_mail_handoffs::{
    MailCancelGroup, MailCancelGroupCause, MailHandoff, MailHandoffItem,
};
pub use project_participant_batch::ParticipantBatch;
pub use project_participant_pages::{ParticipantIntervalBatch, ParticipantIntervalKey};
pub use project_read_store::ProjectReadStore;
mod project_membership_history;
mod project_read_copy;
pub use project_read_copy::ProjectReadCopy;
pub use project_record_batch::StoredProjectRecord;
pub use project_record_reads::LinkMutation;
pub use project_schema::PROJECT_MIGRATOR;
pub use project_schema::StoreSchemaState;
pub use project_schema::{
    PROJECT_SCHEMA_VERSION, check_project_schema, expected_project_fingerprints,
};
pub use project_storage_error::ProjectStorageError;
pub use project_store::{ProjectSnapshot, ProjectStore, ProjectWriteTransaction};
pub use project_subscription_reads::PostActivityKey;
pub use router_local_database::RouterLocalStorageError;

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
mod project_record_size_tests;
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
