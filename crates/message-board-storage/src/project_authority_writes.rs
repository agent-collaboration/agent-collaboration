use crate::project_record_codec::{ProjectRowEnvelope, ProjectRowWrite, require_one_row};
use crate::{ProjectStorageError, ProjectWriteTransaction};
use message_board::*;

impl ProjectWriteTransaction {
    pub(crate) async fn write_role_assignment(
        &mut self,
        row: &RoleAssignment,
        envelope: &ProjectRowEnvelope,
        mode: ProjectRowWrite,
    ) -> Result<(), ProjectStorageError> {
        let holder = self.intern_actor(&row.holder).await?;
        let granted_by = self.intern_actor(&row.granted_by).await?;
        let (project, workstream) = match row.subject() {
            RoleSubject::Project(project) => (Some(String::from(project)), None),
            RoleSubject::Workstream(workstream) => (None, Some(String::from(workstream))),
        };
        let id = row.assignment_id.as_str();
        let actor = envelope.actor_id.as_str();
        let request = envelope.request_id.as_deref();
        let role = row.role().as_str();
        let holder = holder.as_str();
        let granted_by = granted_by.as_str();
        let model = row.model.as_ref().map(ResourceName::as_str);
        let lineage = row.lineage.as_ref().map(ResourceName::as_str);
        let ended = row.ended.as_ref().map(serde_json::to_string).transpose()?;
        let owner_grant = row.owner_grant.as_ref().map(OwnerGrantId::as_str);
        let changed = match mode {
            ProjectRowWrite::Insert => sqlx_turso::query!(
                "INSERT INTO role_assignments(assignment_id, actor_id, request_id, epoch, position, router_time, time_basis, project_subject, workstream_subject, role, holder, granted_by, model, lineage, ended, owner_grant) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                id, actor, request, envelope.epoch, envelope.position, envelope.router_time,
                envelope.time_basis, project, workstream, role, holder, granted_by, model, lineage, ended, owner_grant
            ).execute(&mut self.transaction).await?,
            ProjectRowWrite::Update => sqlx_turso::query!(
                "UPDATE role_assignments SET actor_id=?, request_id=?, epoch=?, position=?, router_time=?, time_basis=?, project_subject=?, workstream_subject=?, role=?, holder=?, granted_by=?, model=?, lineage=?, ended=?, owner_grant=? WHERE assignment_id=?",
                actor, request, envelope.epoch, envelope.position, envelope.router_time,
                envelope.time_basis, project, workstream, role, holder, granted_by, model, lineage, ended, owner_grant, id
            ).execute(&mut self.transaction).await?,
        };
        require_one_row(changed.rows_affected(), row.assignment_id.clone().into())
    }

    pub(crate) async fn write_owner_grant(
        &mut self,
        row: &OwnerGrant,
        envelope: &ProjectRowEnvelope,
    ) -> Result<(), ProjectStorageError> {
        let GrantKind::ReplaceLead {
            project,
            new_holder,
            reason,
        } = &row.grant;
        let OwnerWordsProvenance::Relayed { by } = &row.provenance;
        let new_holder = self.intern_actor(new_holder).await?;
        let relayed_by = self.intern_actor(by).await?;
        let id = row.id.as_str();
        let actor = envelope.actor_id.as_str();
        let request = envelope.request_id.as_deref();
        let project = project.as_str();
        let new_holder = new_holder.as_str();
        let relayed_by = relayed_by.as_str();
        let changed = sqlx_turso::query!(
            "INSERT INTO owner_grants(grant_id, actor_id, request_id, epoch, position, router_time, time_basis, grant_kind, project_id, new_holder, reason, provenance_kind, relayed_by) VALUES (?, ?, ?, ?, ?, ?, ?, 'replaceLead', ?, ?, ?, 'relayed', ?)",
            id, actor, request, envelope.epoch, envelope.position, envelope.router_time,
            envelope.time_basis, project, new_holder, reason, relayed_by
        ).execute(&mut self.transaction).await?;
        require_one_row(changed.rows_affected(), row.id.clone().into())
    }

    pub(crate) async fn write_stand_down_acknowledgement(
        &mut self,
        row: &StandDownAcknowledgement,
        envelope: &ProjectRowEnvelope,
    ) -> Result<(), ProjectStorageError> {
        let id = row.envelope.id.as_str();
        let actor = envelope.actor_id.as_str();
        let request = envelope.request_id.as_deref();
        let assignment = row.assignment.as_str();
        let through_epoch = i64::try_from(row.read_through.epoch.get())
            .map_err(|_| ProjectStorageError::InvalidFeed)?;
        let through_position = i64::try_from(row.read_through.position.get())
            .map_err(|_| ProjectStorageError::InvalidFeed)?;
        let changed = sqlx_turso::query!(
            "INSERT INTO stand_down_acknowledgements(acknowledgement_id, actor_id, request_id, epoch, position, router_time, time_basis, assignment_id, through_epoch, through_position) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            id, actor, request, envelope.epoch, envelope.position, envelope.router_time,
            envelope.time_basis, assignment, through_epoch, through_position
        ).execute(&mut self.transaction).await?;
        require_one_row(
            changed.rows_affected(),
            RecordRef {
                kind: RecordKind::StandDownAcknowledgement,
                id: row.envelope.id.clone(),
            },
        )
    }
}
