//! The owner's outbox rule (2026-11-15): direct messages and thread posts go
//! through the Mac outbox; commands, reads and approvals go immediately.
//! Every arm is spelled out, so a new operation fails to compile until it is
//! classified.
use central_link_protocol::RequestPayload;
use workspace_model::{WorkspaceOperation, central_mail::CentralMailOperation};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubmissionPath {
    /// Stored in the outbox and sent on the next 1-minute mark.
    Outbox,
    /// Sent now over the link; the caller waits for central's answer.
    Immediate,
}

#[must_use]
pub fn submission_path(payload: &RequestPayload) -> SubmissionPath {
    match payload {
        RequestPayload::Workspace(request) => workspace_operation_path(&request.operation),
        RequestPayload::Mail(request) => mail_operation_path(&request.operation),
    }
}

const fn mail_operation_path(operation: &CentralMailOperation) -> SubmissionPath {
    match operation {
        CentralMailOperation::SendMessage { .. } => SubmissionPath::Outbox,
        CentralMailOperation::SetNoticeCadence { .. }
        | CentralMailOperation::SetWake { .. }
        | CentralMailOperation::CancelWake { .. }
        | CentralMailOperation::SetAutomation { .. }
        | CentralMailOperation::DisableAutomation { .. } => SubmissionPath::Immediate,
    }
}

const fn workspace_operation_path(operation: &WorkspaceOperation) -> SubmissionPath {
    match operation {
        WorkspaceOperation::ThreadPost { .. } => SubmissionPath::Outbox,
        WorkspaceOperation::CreateWorkspace { .. }
        | WorkspaceOperation::UpdateWorkspace { .. }
        | WorkspaceOperation::ArchiveWorkspace
        | WorkspaceOperation::CreateArea { .. }
        | WorkspaceOperation::ArchiveArea { .. }
        | WorkspaceOperation::UpdateArea { .. }
        | WorkspaceOperation::CreateProject { .. }
        | WorkspaceOperation::UpdateProject { .. }
        | WorkspaceOperation::UpdateIssue { .. }
        | WorkspaceOperation::CreateInitiative { .. }
        | WorkspaceOperation::UpdateInitiative { .. }
        | WorkspaceOperation::CreateDeliverable { .. }
        | WorkspaceOperation::CreateIssue { .. }
        | WorkspaceOperation::GrantProjectRole { .. }
        | WorkspaceOperation::ReleaseProjectRole { .. }
        | WorkspaceOperation::GrantDeliverableRole { .. }
        | WorkspaceOperation::ReleaseDeliverableRole { .. }
        | WorkspaceOperation::CreateThread { .. }
        | WorkspaceOperation::JoinThread { .. }
        | WorkspaceOperation::AddThreadMember { .. }
        | WorkspaceOperation::LeaveThread { .. }
        | WorkspaceOperation::SetNotifications { .. }
        | WorkspaceOperation::ResolveThread { .. }
        | WorkspaceOperation::ReopenThread { .. }
        | WorkspaceOperation::AddDependency { .. }
        | WorkspaceOperation::RemoveDependency { .. }
        | WorkspaceOperation::SetIssueStatus { .. }
        | WorkspaceOperation::SetProjectStatus { .. }
        | WorkspaceOperation::SetInitiativeStatus { .. }
        | WorkspaceOperation::AddBacklogItem { .. }
        | WorkspaceOperation::UpdateBacklogItem { .. }
        | WorkspaceOperation::RemoveBacklogItem { .. }
        | WorkspaceOperation::CloseBacklogItem { .. }
        | WorkspaceOperation::AddReference { .. }
        | WorkspaceOperation::RemoveReference { .. }
        | WorkspaceOperation::AddLink { .. }
        | WorkspaceOperation::RemoveLink { .. }
        | WorkspaceOperation::WorkLogAdd { .. }
        | WorkspaceOperation::WorkLogRedact { .. }
        | WorkspaceOperation::Show { .. }
        | WorkspaceOperation::List { .. }
        | WorkspaceOperation::Inbox { .. }
        | WorkspaceOperation::MarkRead { .. } => SubmissionPath::Immediate,
    }
}

#[cfg(test)]
#[path = "submission_path_test.rs"]
mod tests;
