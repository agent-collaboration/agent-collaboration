use crate::project_record_codec::{
    ProjectRowEnvelope, ProjectRowWrite, intent_recipient_key, require_one_row,
};
use crate::{ProjectStorageError, ProjectWriteTransaction};
use message_board::{NotifyIntent, RequestLedgerEntry};

impl ProjectWriteTransaction {
    pub(crate) async fn write_notify_intent(
        &mut self,
        row: &NotifyIntent,
        envelope: &ProjectRowEnvelope,
        mode: ProjectRowWrite,
    ) -> Result<(), ProjectStorageError> {
        let id = row.intent_id.as_str();
        let actor = envelope.actor_id.as_str();
        let request = envelope.request_id.as_deref();
        let cause_kind = crate::project_record_io::record_kind_tag(row.cause.kind);
        let cause_id = row.cause.id.as_str();
        let recipient_key = intent_recipient_key(row.recipient(), &row.intent_id.clone().into())?;
        let recipient = serde_json::to_string(row.recipient())?;
        let state = serde_json::to_string(row.state())?;
        let changed = match mode {
            ProjectRowWrite::Insert => sqlx_turso::query!(
                "INSERT INTO notify_intents(intent_id, actor_id, request_id, epoch, position, router_time, time_basis, cause_kind, cause_id, recipient_key, recipient, state) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                id, actor, request, envelope.epoch, envelope.position, envelope.router_time,
                envelope.time_basis, cause_kind, cause_id, recipient_key, recipient, state
            ).execute(&mut self.transaction).await?,
            ProjectRowWrite::Update => sqlx_turso::query!(
                "UPDATE notify_intents SET actor_id=?, request_id=?, epoch=?, position=?, router_time=?, time_basis=?, recipient_key=?, recipient=?, state=? WHERE intent_id=?",
                actor, request, envelope.epoch, envelope.position, envelope.router_time,
                envelope.time_basis, recipient_key, recipient, state, id
            ).execute(&mut self.transaction).await?,
        };
        require_one_row(changed.rows_affected(), row.intent_id.clone().into())
    }

    pub(crate) async fn write_request_decision(
        &mut self,
        row: &RequestLedgerEntry,
        envelope: &ProjectRowEnvelope,
    ) -> Result<(), ProjectStorageError> {
        let id = row.request_id.as_str();
        let actor = envelope.actor_id.as_str();
        let hash = row.canonical_hash.as_str();
        let accepted = row
            .accepted_in
            .map(|epoch| i64::try_from(epoch.get()))
            .transpose()
            .map_err(|_| ProjectStorageError::InvalidFeed)?;
        let decision = serde_json::to_string(&row.decision)?;
        let changed = sqlx_turso::query!(
                "INSERT INTO request_decisions(request_id, actor_id, epoch, position, router_time, time_basis, canonical_hash, accepted_in, decision) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                id, actor, envelope.epoch, envelope.position, envelope.router_time,
                envelope.time_basis, hash, accepted, decision
            ).execute(&mut self.transaction).await?;
        require_one_row(changed.rows_affected(), row.request_id.clone().into())
    }
}
