//! Subject scope and role policy consume only the operands loaded under the project lock.
use super::operation_context::{OperationContext, OperationPlan};
use message_board::{
    Record, RecordId, Refusal, RoleRefusal, RoleRequirement, ThreadId, ThreadRefiled, ThreadSubject,
};

impl OperationContext<'_> {
    pub(super) fn require_project_role(&self) -> Result<(), Refusal> {
        if self.records.iter().any(|record| matches!(record, Record::RoleAssignment(role) if role.ended.is_none() && role.holder.declared_identity().as_ref() == Some(&self.request.actor))) {
            Ok(())
        } else {
            Err(Refusal::Role(RoleRefusal::RoleRequired { requirement: RoleRequirement::AnyProjectRole }))
        }
    }
    pub(super) fn validate_thread_subject(&self, subject: &ThreadSubject) -> Result<(), Refusal> {
        match subject {
            ThreadSubject::Project { .. } => Ok(()),
            ThreadSubject::Workstream(id) => self.record(&id.clone().into()).map(|_| ()),
            ThreadSubject::Task(id) => self.record(&id.clone().into()).map(|_| ()),
            ThreadSubject::WorkTrailRow(id) => self.record(&id.clone().into()).map(|_| ()),
            ThreadSubject::WorkspaceAdr(id) => {
                let adr = self
                    .workspace_adrs
                    .iter()
                    .find(|adr| adr.adr_id == *id)
                    .ok_or_else(|| Refusal::NotFound(id.clone().into()))?;
                if adr.workspace != self.scope.workspace {
                    return Err(Refusal::InvalidShape {
                        field: "about".into(),
                    });
                }
                Ok(())
            }
        }
    }
    pub(super) fn refile_thread_plan(
        &self,
        thread: &ThreadId,
        about: &ThreadSubject,
    ) -> Result<OperationPlan, Refusal> {
        self.require_project_role()?;
        let mut current = self.thread(thread)?;
        self.validate_thread_subject(about)?;
        if current.about == *about {
            return Ok(OperationPlan::unchanged());
        }
        let event = ThreadRefiled {
            envelope: self.envelope(RecordId::generate()),
            thread: thread.clone(),
            from: current.about,
            to: about.clone(),
        };
        current.about = about.clone();
        current.envelope = self.envelope(thread.clone().into());
        Ok(OperationPlan::changed(vec![
            Record::Thread(current),
            Record::ThreadRefiled(event),
        ]))
    }
}
