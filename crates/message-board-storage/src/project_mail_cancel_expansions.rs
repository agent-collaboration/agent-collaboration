//! Child controls retain their frozen bytes and their own replicated commit order.
use crate::{
    MailHandoffItem, ProjectSnapshot, ProjectStorageError, ProjectWriteTransaction,
    project_mail_handoffs::handoff,
};
use board_sync_protocol::{BoardDeliveryBody, BoardDeliveryTypes};
use collaboration_mail_protocol::NotificationSource;
use fleet_sync_protocol::DeliveryEnvelope;
use message_board::{CancelExpansionSeq, MailItemRef, ProjectId};
use sqlx::Executor;
use sqlx_turso::Turso;

pub struct MailCancelExpansion {
    pub group_handoff: MailItemRef,
    pub target_item: MailItemRef,
    pub envelope: Box<DeliveryEnvelope<BoardDeliveryTypes>>,
}

pub struct SequencedMailCancelExpansion {
    pub sequence: CancelExpansionSeq,
    pub expansion: MailCancelExpansion,
}

impl MailCancelExpansion {
    pub fn id(&self) -> Result<MailItemRef, ProjectStorageError> {
        MailItemRef::try_from(self.envelope.id().as_uuid().to_string())
            .map_err(|_| ProjectStorageError::InvalidFeed)
    }

    async fn validate<TScope>(
        &self,
        scope: &mut TScope,
        project: &ProjectId,
    ) -> Result<(), ProjectStorageError>
    where
        for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
    {
        let DeliveryEnvelope::Cancel(cancel) = &*self.envelope else {
            return Err(ProjectStorageError::InvalidFeed);
        };
        let group = handoff(scope, project, &self.group_handoff)
            .await?
            .ok_or(ProjectStorageError::InvalidFeed)?;
        let target = handoff(scope, project, &self.target_item)
            .await?
            .ok_or(ProjectStorageError::InvalidFeed)?;
        let MailHandoffItem::CancelGroup(group_item) = group.item else {
            return Err(ProjectStorageError::InvalidFeed);
        };
        let MailHandoffItem::Delivery(target_envelope) = target.item else {
            return Err(ProjectStorageError::InvalidFeed);
        };
        let DeliveryEnvelope::Item(target_item) = &*target_envelope else {
            return Err(ProjectStorageError::InvalidFeed);
        };
        let BoardDeliveryBody::Notification(notification) = target_item.body() else {
            return Err(ProjectStorageError::InvalidFeed);
        };
        let NotificationSource::FollowBatch {
            membership,
            activated_at,
            generation,
            ..
        } = &notification.source
        else {
            return Err(ProjectStorageError::InvalidFeed);
        };
        if target.sequence >= group.sequence
            || membership != &group_item.membership
            || activated_at != &group_item.activated_at
            || generation != &group_item.generation
            || cancel.target() != target_item.id()
            || cancel.sender() != target_item.sender()
            || cancel.recipient() != target_item.recipient()
            || self.id()? == self.group_handoff
            || self.id()? == self.target_item
        {
            return Err(ProjectStorageError::InvalidFeed);
        }
        Ok(())
    }
}

struct ExpansionRow {
    cancel_id: String,
    expansion_seq: i64,
    group_handoff: String,
    target_item: String,
    envelope: String,
}

impl ExpansionRow {
    async fn decode<TScope>(
        self,
        scope: &mut TScope,
        project: &ProjectId,
    ) -> Result<SequencedMailCancelExpansion, ProjectStorageError>
    where
        for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
    {
        let invalid = || ProjectStorageError::InvalidFeed;
        let id = MailItemRef::try_from(self.cancel_id).map_err(|_| invalid())?;
        let sequence =
            CancelExpansionSeq::try_from(u64::try_from(self.expansion_seq).map_err(|_| invalid())?)
                .map_err(|_| invalid())?;
        if sequence == CancelExpansionSeq::ZERO {
            return Err(invalid());
        }
        let expansion = MailCancelExpansion {
            group_handoff: self.group_handoff.try_into().map_err(|_| invalid())?,
            target_item: self.target_item.try_into().map_err(|_| invalid())?,
            envelope: serde_json::from_str(&self.envelope).map_err(|_| invalid())?,
        };
        if expansion.id()? != id {
            return Err(invalid());
        }
        expansion.validate(scope, project).await?;
        Ok(SequencedMailCancelExpansion {
            sequence,
            expansion,
        })
    }
}

async fn expansion_for_target<TScope>(
    scope: &mut TScope,
    project: &ProjectId,
    group: &MailItemRef,
    target: &MailItemRef,
) -> Result<Option<SequencedMailCancelExpansion>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    let row = sqlx_turso::query_as!(ExpansionRow, r#"SELECT cancel_id AS "cancel_id!: String",expansion_seq AS "expansion_seq!: i64",group_handoff AS "group_handoff!: String",target_item AS "target_item!: String",envelope AS "envelope!: String" FROM mail_cancel_expansions WHERE group_handoff=? AND target_item=?"#, group.as_str(), target.as_str())
        .fetch_optional(&mut *scope).await?;
    match row {
        Some(row) => row.decode(scope, project).await.map(Some),
        None => Ok(None),
    }
}

impl ProjectWriteTransaction {
    /// Exact replay keeps the original child; a different identity or body is refused.
    pub async fn append_mail_cancel_expansion(
        &mut self,
        expansion: &MailCancelExpansion,
    ) -> Result<CancelExpansionSeq, ProjectStorageError> {
        expansion
            .validate(&mut self.transaction, &self.project_id)
            .await?;
        let id = expansion.id()?;
        let envelope = serde_json::to_string(&expansion.envelope)?;
        if let Some(existing) = self
            .mail_cancel_expansion_for_target(&expansion.group_handoff, &expansion.target_item)
            .await?
        {
            if existing.expansion.id()? != id
                || serde_json::to_string(&existing.expansion.envelope)? != envelope
            {
                return Err(ProjectStorageError::InvalidFeed);
            }
            return Ok(existing.sequence);
        }
        let highest = sqlx_turso::query_scalar!(
            r#"SELECT MAX(expansion_seq) AS "highest?: i64" FROM mail_cancel_expansions"#
        )
        .fetch_one(&mut self.transaction)
        .await?
        .unwrap_or(0);
        let sequence = CancelExpansionSeq::try_from(
            u64::try_from(highest).map_err(|_| ProjectStorageError::InvalidFeed)?,
        )
        .map_err(|_| ProjectStorageError::InvalidFeed)?
        .successor()
        .map_err(|_| ProjectStorageError::InvalidFeed)?;
        let ordinal =
            i64::try_from(sequence.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        sqlx_turso::query!(
            "INSERT INTO mail_cancel_expansions(cancel_id,expansion_seq,group_handoff,target_item,envelope) VALUES(?,?,?,?,?)",
            id.as_str(), ordinal, expansion.group_handoff.as_str(), expansion.target_item.as_str(), envelope
        ).execute(&mut self.transaction).await?;
        Ok(sequence)
    }

    pub async fn mail_cancel_expansion_for_target(
        &mut self,
        group: &MailItemRef,
        target: &MailItemRef,
    ) -> Result<Option<SequencedMailCancelExpansion>, ProjectStorageError> {
        expansion_for_target(&mut self.transaction, &self.project_id, group, target).await
    }
}

impl ProjectSnapshot {
    pub async fn mail_cancel_expansion_for_target(
        &mut self,
        group: &MailItemRef,
        target: &MailItemRef,
    ) -> Result<Option<SequencedMailCancelExpansion>, ProjectStorageError> {
        expansion_for_target(&mut self.transaction, &self.project_id, group, target).await
    }

    pub async fn mail_cancel_expansion_page(
        &mut self,
        after: CancelExpansionSeq,
        limit: u64,
    ) -> Result<Vec<SequencedMailCancelExpansion>, ProjectStorageError> {
        let after = i64::try_from(after.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let limit = i64::try_from(limit).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let rows = sqlx_turso::query_as!(ExpansionRow, r#"SELECT cancel_id AS "cancel_id!: String",expansion_seq AS "expansion_seq!: i64",group_handoff AS "group_handoff!: String",target_item AS "target_item!: String",envelope AS "envelope!: String" FROM mail_cancel_expansions WHERE expansion_seq>? ORDER BY expansion_seq LIMIT ?"#, after, limit)
            .fetch_all(&mut self.transaction).await?;
        let mut result = Vec::with_capacity(rows.len());
        for row in rows {
            result.push(row.decode(&mut self.transaction, &self.project_id).await?);
        }
        Ok(result)
    }
}
