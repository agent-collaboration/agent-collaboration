//! Catalog, source execution, and native launch inventory.

#[path = "session_commands/session_launch_selection.rs"]
pub mod session_launch_selection;
pub use session_launch_selection::SessionsLaunchTarget;
#[path = "session_commands/claude_launch_target.rs"]
pub mod claude_launch_target;
#[path = "session_commands/claude_session_commands.rs"]
pub mod claude_session_commands;
pub use claude_session_commands::run_claude_sessions_command;

pub use collaboration_client::session_catalog::SessionSearchExpression;

#[path = "session_commands/session_command_options.rs"]
pub mod session_command_options;
pub use session_command_options::validate_exact_uuid_session_id;
pub use session_command_options::{
    SessionsCommand, SessionsFormat, SessionsProvider, SessionsRoot, SessionsSort, SessionsSource,
    command_help,
};
#[path = "session_commands/session_command_failures.rs"]
pub mod session_command_failures;
pub use collaboration_client::session_catalog::{
    RepositoryIdentity, normalized_paths_resolve_to_same_location,
    repository_contains_session as session_belongs_to_repository,
};
pub use collaboration_client::session_catalog::{discover_repository_identity, normalize_path};
pub use session_command_failures::SessionsCommandError;
#[path = "session_commands/session_display_text.rs"]
pub mod session_display_text;
#[cfg(test)]
pub use session_display_text::format_duration_ms;
#[cfg(test)]
pub use session_display_text::session_model_choice;
pub use session_display_text::{
    display_title_from_session_fields, format_recency_at_ms, human_session_row,
    session_context_from_cwd, truncate_end,
};
#[path = "session_commands/session_catalog_records.rs"]
pub mod session_catalog_records;
pub use session_catalog_records::SessionRecord;
pub use session_catalog_records::{
    SessionConversationPreview, SessionConversationSource, SessionPickerIdentity,
    SessionPickerRecord, SessionRowProvenance,
};
#[path = "session_commands/session_action_selection.rs"]
pub mod session_action_selection;
pub use session_action_selection::SessionActionSelection;
#[path = "session_commands/session_catalog_query.rs"]
pub mod session_catalog_query;
pub use codex_native_integration::ResumeModelChoice;
pub use session_catalog_query::{
    SessionRecordQuery, load_session_records, load_session_records_for_query_with_identity,
};

#[path = "session_commands/router_connection_registry.rs"]
pub mod router_connection_registry;
pub use router_connection_registry::{RouterRegistryError, RouterRegistryRead};

#[path = "session_commands/picker_runtime_inventory.rs"]
pub mod picker_runtime_inventory;

#[path = "session_commands/configured_inventory_loader.rs"]
pub mod configured_inventory_loader;
#[path = "session_commands/configured_inventory_query.rs"]
pub mod configured_inventory_query;
#[path = "session_commands/configured_inventory_transport.rs"]
pub mod configured_inventory_transport;
#[path = "session_commands/source_inventory_failure.rs"]
pub mod source_inventory_failure;

#[path = "session_commands/source_native_carrier.rs"]
pub mod source_native_carrier;
#[path = "session_commands/source_native_options.rs"]
pub mod source_native_options;
#[path = "session_commands/source_ssh_executor.rs"]
pub mod source_ssh_executor;
#[path = "session_commands/source_ssh_protocol.rs"]
pub mod source_ssh_protocol;
#[path = "session_commands/source_ssh_transport.rs"]
pub mod source_ssh_transport;

#[path = "session_commands/source_action_transport.rs"]
pub mod source_action_transport;
#[path = "session_commands/source_client_action.rs"]
pub mod source_client_action;

#[cfg(test)]
#[path = "../tests/configured_inventory_faults.rs"]
pub mod configured_inventory_fault_tests;
#[cfg(test)]
#[path = "../tests/configured_inventory_read.rs"]
pub mod configured_inventory_read_tests;

#[path = "session_commands/native_process_runner.rs"]
pub mod native_process_runner;
pub use native_process_runner::ProcessSessionsCommandRunner;
pub use native_process_runner::SessionsCommandRunner;

const SESSION_TITLE_MAX_CHARS: usize = 96;
const SESSION_CONTEXT_MAX_CHARS: usize = 32;
const SESSION_MODEL_CHOICE_MAX_CHARS: usize = 32;
const SESSION_CONVERSATION_SNIPPET_MAX_CHARS: usize = 180;
const DEFAULT_SESSION_RECORD_LIMIT: usize = 100;

/// Projects one catalog record into the launch-time model choice, warning once when a
/// stored value cannot be quoted as a TOML basic string.
pub fn stored_model_choice_from_record(record: &SessionRecord) -> ResumeModelChoice {
    let model = record.model.as_deref();
    let reasoning_effort = record.reasoning_effort.as_deref();
    if ResumeModelChoice::rejects_stored_value(model, reasoning_effort) {
        eprintln!(
            "agent-sessions: stored model or reasoning effort for session {} contains characters that cannot be passed to Codex; resuming without it",
            record.session_id
        );
    }
    ResumeModelChoice::from_stored_values(model, reasoning_effort)
}

pub fn validate_resume_session_id(session_id: &str) -> Result<(), SessionsCommandError> {
    let trimmed = session_id.trim();
    if trimmed.is_empty()
        || trimmed != session_id
        || trimmed.starts_with('-')
        || trimmed
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err(SessionsCommandError::UnsafeSessionId);
    }
    Ok(())
}

#[cfg(test)]
#[path = "session_commands/session_projection_tests.rs"]
pub mod tests;

use crate::picker_data_query::{SessionsPickerDataQuery, SessionsPickerRoot};

#[cfg(test)]
use crate::session_picker_record_loader::session_picker_record_loader;
#[cfg(test)]
use crate::test_support::fake_api_support;
