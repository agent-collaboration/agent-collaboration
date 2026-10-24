use crate::project_record_codec::{
    ProjectRowEnvelope, ProjectRowWrite, require_one_row, timestamp,
};
use crate::{ProjectStorageError, ProjectWriteTransaction};
use message_board::*;

impl ProjectWriteTransaction {
    pub(crate) async fn write_membership(
        &mut self,
        row: &SubscriptionMembership,
        envelope: &ProjectRowEnvelope,
        mode: ProjectRowWrite,
    ) -> Result<(), ProjectStorageError> {
        let id = row.envelope.id.as_str();
        let actor = envelope.actor_id.as_str();
        let request = envelope.request_id.as_deref();
        let reader = self
            .intern_actor(&RecordedActor::declared(row.reader().clone()))
            .await?;
        let reader = reader.as_str();
        let (scope_kind, scope_id) = row.scope().kind_and_id();
        let policy = serde_json::to_string(row.policy())?;
        let renewed = timestamp(row.renewed_at());
        let expires = timestamp(row.expires_at());
        let generation =
            i64::try_from(row.generation().get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let activated_epoch = i64::try_from(row.activated_at().epoch.get())
            .map_err(|_| ProjectStorageError::InvalidFeed)?;
        let activated_position = i64::try_from(row.activated_at().position.get())
            .map_err(|_| ProjectStorageError::InvalidFeed)?;
        let ended = row.ended().map(serde_json::to_string).transpose()?;
        let changed = match mode {
            ProjectRowWrite::Insert => sqlx_turso::query!(
                "INSERT INTO subscription_memberships(membership_id, actor_id, request_id, epoch, position, router_time, time_basis, reader_actor, scope_kind, scope_id, policy, renewed_at, expires_at, generation, activated_epoch, activated_position, ended) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                id, actor, request, envelope.epoch, envelope.position, envelope.router_time,
                envelope.time_basis, reader, scope_kind, scope_id, policy, renewed, expires,
                generation, activated_epoch, activated_position, ended
            ).execute(&mut self.transaction).await?,
            ProjectRowWrite::Update => sqlx_turso::query!(
                "UPDATE subscription_memberships SET actor_id=?, request_id=?, epoch=?, position=?, router_time=?, time_basis=?, policy=?, renewed_at=?, expires_at=?, generation=?, activated_epoch=?, activated_position=?, ended=? WHERE membership_id=? AND reader_actor=? AND scope_kind=? AND scope_id=?",
                actor, request, envelope.epoch, envelope.position, envelope.router_time,
                envelope.time_basis, policy, renewed, expires, generation, activated_epoch, activated_position,
                ended, id, reader, scope_kind, scope_id
            ).execute(&mut self.transaction).await?,
        };
        require_one_row(
            changed.rows_affected(),
            RecordRef {
                kind: RecordKind::SubscriptionMembership,
                id: row.envelope.id.clone(),
            },
        )
    }
}
