//! Original test roots retain their case namespaces.

#[path = "../support/wake_push_draft.rs"]
mod wake_push_test_support;

mod automation_collection_pages;
mod automation_event_history;
mod automation_event_retention;
mod automation_extended_retention;
mod automation_retention;
mod commit_signal;
mod delivery_claim_concurrency;
mod delivery_outcome_rules;
mod delivery_partial_uncertainty;
mod delivery_restart_recovery;
mod instruction_history;
mod instruction_replay;
mod message_drop;
mod operation_receipt_history;
mod preparation_operation_replay;
mod prepared_thread_commit;
mod remote_wake_routes;
mod route_delivery_transitions;
mod run_admission_concurrency;
mod run_completion_identity;
mod run_dispatch_budget;
mod run_preparation_recovery;
mod run_route_transitions;
mod schedule_catchup;
mod schedule_creation;
mod schedule_definition_edits;
mod schedule_import_continuity;
mod schedule_package_import;
mod scheduler_inventory_progress;
mod schema_initialization;
mod summary_result_identity;
mod summary_retry_identity;
mod summary_run_occupancy;
mod thread_binding_ownership;
mod wake_rules;
mod wake_slot_upgrade;
mod wakeup_creation;
mod wakeup_firing;
mod wakeup_lifecycle;
mod wakeup_list_pagination;
mod wakeup_transition_ordering;
mod wakeup_worker_inventory;
