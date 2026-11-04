//! Original test roots retain their case namespaces.

#[path = "../project_storage_support/mod.rs"]
mod project_storage_support;

mod board_project_schema;
mod capture_annotations;
mod fleet_service_session;
mod machine_notice_progress_store;
mod machine_session_store;
mod project_conversation_storage;
mod project_cursors;
mod project_merge_time_storage;
mod project_storage;
mod reqwest_session_transport;
