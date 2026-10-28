//! Replicated notification allocations retain the original SDK identity and bytes.
use crate::{ProjectSnapshot, ProjectStorageError, ProjectWriteTransaction};
use board_sync_protocol::{BoardDeliveryBody, BoardDeliveryTypes};
use fleet_sync_protocol::DeliveryEnvelope;
use message_board::{
    EndReason, FeedCursor, HandoffSeq, MailItemRef, ProjectId, SubscriptionGeneration,
};
use serde::{Deserialize, Serialize};
use sqlx::Executor;
use sqlx_turso::Turso;

#[derive(Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum MailHandoffItem {
    Delivery(Box<DeliveryEnvelope<BoardDeliveryTypes>>),
    CancelGroup(MailCancelGroup),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MailCancelGroup {
    pub id: MailItemRef,
    pub membership: collaboration_mail_protocol::MembershipId,
    pub activated_at: FeedCursor,
    pub generation: SubscriptionGeneration,
    pub cause: MailCancelGroupCause,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum MailCancelGroupCause {
    GenerationChanged,
    MembershipEnded { reason: EndReason },
}

pub struct MailHandoff {
    pub sequence: HandoffSeq,
    pub item: MailHandoffItem,
}

impl MailHandoffItem {
    pub fn id(&self) -> Result<MailItemRef, ProjectStorageError> {
        match self {
            Self::Delivery(envelope) => MailItemRef::try_from(envelope.id().as_uuid().to_string())
                .map_err(|_| ProjectStorageError::InvalidFeed),
            Self::CancelGroup(group) => Ok(group.id.clone()),
        }
    }

    fn validate_project(&self, project: &ProjectId) -> Result<(), ProjectStorageError> {
        if let Self::Delivery(envelope) = self {
            let collaboration_mail_protocol::MailSender::Project { project: sender } =
                envelope.sender()
            else {
                return Err(ProjectStorageError::InvalidFeed);
            };
            if sender != project {
                return Err(ProjectStorageError::InvalidFeed);
            }
            if let DeliveryEnvelope::Item(item) = &**envelope {
                item.body()
                    .validate_sender(item.sender())
                    .map_err(|_| ProjectStorageError::InvalidFeed)?;
                let BoardDeliveryBody::Notification(body) = item.body() else {
                    return Err(ProjectStorageError::InvalidFeed);
                };
                if &body.pointer.project != project {
                    return Err(ProjectStorageError::InvalidFeed);
                }
            }
        }
        Ok(())
    }
}

struct HandoffRow {
    item_id: String,
    handoff_seq: i64,
    item: String,
}

impl HandoffRow {
    fn decode(self, project: &ProjectId) -> Result<MailHandoff, ProjectStorageError> {
        let id =
            MailItemRef::try_from(self.item_id).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let sequence = HandoffSeq::try_from(
            u64::try_from(self.handoff_seq).map_err(|_| ProjectStorageError::InvalidFeed)?,
        )
        .map_err(|_| ProjectStorageError::InvalidFeed)?;
        if sequence == HandoffSeq::ZERO {
            return Err(ProjectStorageError::InvalidFeed);
        }
        let item: MailHandoffItem =
            serde_json::from_str(&self.item).map_err(|_| ProjectStorageError::InvalidFeed)?;
        item.validate_project(project)?;
        if item.id()? != id {
            return Err(ProjectStorageError::InvalidFeed);
        }
        Ok(MailHandoff { sequence, item })
    }
}

pub(super) async fn handoff<TScope>(
    scope: &mut TScope,
    project: &ProjectId,
    id: &MailItemRef,
) -> Result<Option<MailHandoff>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    sqlx_turso::query_file_as!(HandoffRow, "src/project_mail_handoff.sql", id.as_str())
        .fetch_optional(scope)
        .await?
        .map(|row| row.decode(project))
        .transpose()
}

impl ProjectWriteTransaction {
    /// A committed allocation can replay exactly; it can never change its content.
    pub async fn append_mail_handoff(
        &mut self,
        item: &MailHandoffItem,
    ) -> Result<HandoffSeq, ProjectStorageError> {
        item.validate_project(&self.project_id)?;
        let id = item.id()?;
        let encoded = serde_json::to_string(item)?;
        if let Some(existing) = self.mail_handoff(&id).await? {
            if serde_json::to_string(&existing.item)? != encoded {
                return Err(ProjectStorageError::InvalidFeed);
            }
            return Ok(existing.sequence);
        }
        let highest = sqlx_turso::query_scalar!(
            r#"SELECT MAX(handoff_seq) AS "highest?: i64" FROM mail_handoffs"#
        )
        .fetch_one(&mut self.transaction)
        .await?
        .unwrap_or(0);
        let sequence = HandoffSeq::try_from(
            u64::try_from(highest).map_err(|_| ProjectStorageError::InvalidFeed)?,
        )
        .map_err(|_| ProjectStorageError::InvalidFeed)?
        .successor()
        .map_err(|_| ProjectStorageError::InvalidFeed)?;
        let ordinal =
            i64::try_from(sequence.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        sqlx_turso::query!(
            "INSERT INTO mail_handoffs(item_id,handoff_seq,item) VALUES (?,?,?)",
            id.as_str(),
            ordinal,
            encoded
        )
        .execute(&mut self.transaction)
        .await?;
        Ok(sequence)
    }

    pub async fn mail_handoff(
        &mut self,
        id: &MailItemRef,
    ) -> Result<Option<MailHandoff>, ProjectStorageError> {
        handoff(&mut self.transaction, &self.project_id, id).await
    }
}

impl ProjectSnapshot {
    pub async fn mail_handoff_head(&mut self) -> Result<HandoffSeq, ProjectStorageError> {
        handoff_head(&mut self.transaction).await
    }
    pub async fn mail_settled_through(&mut self) -> Result<HandoffSeq, ProjectStorageError> {
        settled_through(&mut self.transaction).await
    }
    pub async fn mail_handoff(
        &mut self,
        id: &MailItemRef,
    ) -> Result<Option<MailHandoff>, ProjectStorageError> {
        handoff(&mut self.transaction, &self.project_id, id).await
    }

    pub async fn mail_handoff_page(
        &mut self,
        after: HandoffSeq,
        limit: u64,
    ) -> Result<Vec<MailHandoff>, ProjectStorageError> {
        handoff_page(&mut self.transaction, &self.project_id, after, limit).await
    }
}

impl ProjectWriteTransaction {
    pub async fn mail_handoff_head(&mut self) -> Result<HandoffSeq, ProjectStorageError> {
        handoff_head(&mut self.transaction).await
    }

    /// Admission owns the proof of settlement; storage enforces a valid monotonic prefix.
    pub async fn advance_mail_settled_through(
        &mut self,
        through: HandoffSeq,
    ) -> Result<bool, ProjectStorageError> {
        let current = self.mail_settled_through().await?;
        if through < current || through > self.mail_handoff_head().await? {
            return Err(ProjectStorageError::InvalidFeed);
        }
        if through == current {
            return Ok(false);
        }
        let ordinal = i64::try_from(through.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        if current == HandoffSeq::ZERO {
            // A zero singleton is valid after a seed; preserve that identity too.
            let updated =
                sqlx_turso::query!("UPDATE handoff_frontier SET settled_through=?", ordinal)
                    .execute(&mut self.transaction)
                    .await?;
            if updated.rows_affected() == 0 {
                let id = uuid::Uuid::now_v7().to_string();
                sqlx_turso::query!(
                    "INSERT INTO handoff_frontier(frontier_id,settled_through) VALUES(?,?)",
                    id,
                    ordinal
                )
                .execute(&mut self.transaction)
                .await?;
            }
        } else {
            sqlx_turso::query!("UPDATE handoff_frontier SET settled_through=?", ordinal)
                .execute(&mut self.transaction)
                .await?;
        }
        Ok(true)
    }
    pub async fn mail_settled_through(&mut self) -> Result<HandoffSeq, ProjectStorageError> {
        settled_through(&mut self.transaction).await
    }
    pub async fn mail_handoff_page(
        &mut self,
        after: HandoffSeq,
        limit: u64,
    ) -> Result<Vec<MailHandoff>, ProjectStorageError> {
        handoff_page(&mut self.transaction, &self.project_id, after, limit).await
    }
}

async fn handoff_head<TScope>(scope: &mut TScope) -> Result<HandoffSeq, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    let highest = sqlx_turso::query_scalar!(
        r#"SELECT MAX(handoff_seq) AS "highest?: i64" FROM mail_handoffs"#
    )
    .fetch_one(&mut *scope)
    .await?
    .unwrap_or(0);
    HandoffSeq::try_from(u64::try_from(highest).map_err(|_| ProjectStorageError::InvalidFeed)?)
        .map_err(|_| ProjectStorageError::InvalidFeed)
}

async fn handoff_page<TScope>(
    transaction: &mut TScope,
    project: &ProjectId,
    after: HandoffSeq,
    limit: u64,
) -> Result<Vec<MailHandoff>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    let after = i64::try_from(after.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
    let limit = i64::try_from(limit).map_err(|_| ProjectStorageError::InvalidFeed)?;
    let rows = sqlx_turso::query_as!(HandoffRow, r#"SELECT item_id AS "item_id!: String", handoff_seq AS "handoff_seq!: i64", item AS "item!: String" FROM mail_handoffs WHERE handoff_seq>? ORDER BY handoff_seq LIMIT ?"#, after, limit)
        .fetch_all(&mut *transaction).await?;
    rows.into_iter().map(|row| row.decode(project)).collect()
}

struct FrontierRow {
    frontier_id: String,
    settled_through: i64,
}

async fn settled_through<TScope>(scope: &mut TScope) -> Result<HandoffSeq, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    let rows = sqlx_turso::query_as!(FrontierRow,
        r#"SELECT frontier_id AS "frontier_id!: String",settled_through AS "settled_through!: i64" FROM handoff_frontier LIMIT 2"#
    ).fetch_all(&mut *scope).await?;
    if rows.is_empty() {
        return Ok(HandoffSeq::ZERO);
    }
    let [row] = rows.as_slice() else {
        return Err(ProjectStorageError::InvalidFeed);
    };
    let id =
        uuid::Uuid::parse_str(&row.frontier_id).map_err(|_| ProjectStorageError::InvalidFeed)?;
    if id.get_version_num() != 7
        || id.get_variant() != uuid::Variant::RFC4122
        || id.hyphenated().to_string() != row.frontier_id
    {
        return Err(ProjectStorageError::InvalidFeed);
    }
    let position = HandoffSeq::try_from(
        u64::try_from(row.settled_through).map_err(|_| ProjectStorageError::InvalidFeed)?,
    )
    .map_err(|_| ProjectStorageError::InvalidFeed)?;
    let highest = sqlx_turso::query_scalar!(
        r#"SELECT MAX(handoff_seq) AS "highest?: i64" FROM mail_handoffs"#
    )
    .fetch_one(&mut *scope)
    .await?
    .unwrap_or(0);
    if position.get() > u64::try_from(highest).map_err(|_| ProjectStorageError::InvalidFeed)? {
        return Err(ProjectStorageError::InvalidFeed);
    }
    Ok(position)
}
