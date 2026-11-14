//! Closed workspace vocabulary. Identity, transport and storage have other owners.
mod entity_records;
mod item_references;
mod request_contract;
mod rule_commands;
mod rule_results;
mod validated_values;
pub use request_contract::*;

pub use entity_records::*;
pub use item_references::*;
pub use rule_commands::*;
pub use rule_results::*;
pub use validated_values::*;

/// Opaque details remain JSON; command payloads and states stay typed.
pub fn encode_json(value: &impl serde::Serialize) -> Result<String, ValueValidationError> {
    serde_json::to_string(value).map_err(|_| ValueValidationError {
        field: "json",
        requirement: "cannot be encoded",
    })
}
pub use workspace_ids::{
    ActorIdentityId, AreaId, BacklogItemId, ChangeId, DeliverableId, DeliverableRoleId,
    DependencyId, InitiativeId, IssueId, LinkId, MachineId, PersonId, PostId, ProjectId,
    ProjectRoleId, ReadMarkId, ReferenceId, RequestId, ThreadId, ThreadMemberId, WorkLogEntryId,
    WorkLogTagId, WorkspaceId,
};
