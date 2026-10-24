//! Each record family writes its owning typed table through the admitted library scope.
use crate::project_record_codec::{ProjectRowEnvelope, ProjectRowWrite};
use crate::{ProjectStorageError, ProjectWriteTransaction};
use message_board::Record;

impl ProjectWriteTransaction {
    pub(crate) async fn write_project_record(
        &mut self,
        record: &Record,
        envelope: &ProjectRowEnvelope,
        mode: ProjectRowWrite,
    ) -> Result<(), ProjectStorageError> {
        match record {
            Record::Project(row) => self.write_project_state(row, envelope, mode).await,
            Record::Workstream(row) => self.write_workstream(row, envelope, mode).await,
            Record::Task(row) => self.write_task(row, envelope, mode).await,
            Record::Subtask(row) => self.write_subtask(row, envelope, mode).await,
            Record::Thread(row) => self.write_thread(row, envelope, mode).await,
            Record::ThreadRefiled(row) => self.write_thread_refiling(row, envelope).await,
            Record::Post(row) => self.write_post(row, envelope).await,
            Record::RoleAssignment(row) => self.write_role_assignment(row, envelope, mode).await,
            Record::OwnerGrant(row) => self.write_owner_grant(row, envelope).await,
            Record::StandDownAcknowledgement(row) => {
                self.write_stand_down_acknowledgement(row, envelope).await
            }
            Record::Move(row) => self.write_move(row, envelope).await,
            Record::Fact(row) => self.write_fact(row, envelope).await,
            Record::PlanRevision(row) => self.write_plan_revision(row, envelope, mode).await,
            Record::NotifyIntent(row) => self.write_notify_intent(row, envelope, mode).await,
            Record::SubscriptionMembership(row) => self.write_membership(row, envelope, mode).await,
            Record::Request(row) => self.write_request_decision(row, envelope).await,
            Record::TaskLinkEvent(row) => self.write_task_link_event(row, envelope).await,
            Record::WorkTrailRow(row) => self.write_trail_row(row, envelope).await,
            Record::TrailRedaction(row) => self.write_trail_redaction(row).await,
            Record::ReadMark(_)
            | Record::Workspace(_)
            | Record::Board(_)
            | Record::Adr(_)
            | Record::ProcessDefinition(_)
            | Record::DirectoryEntry(_) => Err(ProjectStorageError::InvalidRecord {
                record: record.reference(),
                field: "scope",
            }),
        }
    }
}
