//! Scoped trail and historical-authority metadata for the view projection owner.
use crate::project_record_io::validate_canonical_project_text;
use crate::{ProjectSnapshot, ProjectStorageError};
use message_board::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrailActivityMetadata {
    pub row_id: WorkTrailRowId,
    id: RecordId,
    project: ProjectId,
    scope: ProjectId,
    position: RecordPosition,
    actor: ActorKind,
    pub subject: TrailSubject,
    pub tags: Vec<TrailTag>,
    pub links: Vec<EvidencePointer>,
}
impl TrailActivityMetadata {
    pub fn position(&self) -> Result<FeedCursor, ProjectStorageError> {
        match self.position {
            RecordPosition::Project(cursor) => Ok(cursor),
            _ => Err(ProjectStorageError::InvalidFeed),
        }
    }
    pub fn declared_actor(&self) -> Option<&Identity> {
        match &self.actor {
            ActorKind::Declared(actor) => Some(actor),
            ActorKind::Router { .. } => None,
        }
    }
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.iter().any(|item| item.as_str() == tag)
    }
}
#[derive(Clone, Copy)]
pub enum PendingTrailSelection {
    Questions,
    Handbacks,
    Both,
}
#[derive(Default)]
pub struct PendingTrailInputs {
    pub candidates: Vec<TrailActivityMetadata>,
    pub decisions: Vec<TrailDecisionMetadata>,
}
pub struct TrailDecisionMetadata {
    pub row: TrailActivityMetadata,
    pub authority: Option<HistoricalLeadMetadata>,
}
pub struct HistoricalLeadMetadata {
    pub holder: Identity,
    pub ended: Option<RoleEnd>,
}
struct PendingCardScope<'request> {
    subject: &'request TrailSubject,
    handbacks_by: Option<&'request Identity>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DecisionAuthority {
    role: AuthorityRole,
    subject: RoleSubject,
    holder: ActorKind,
    ended: Option<RoleEnd>,
}
fn decode_metadata(
    payload: &str,
    project: &ProjectId,
) -> Result<TrailActivityMetadata, ProjectStorageError> {
    tracing::trace!(target: "message_board_storage::decode", decode_kind = "trailMetadata");
    let row: TrailActivityMetadata = serde_json::from_str(payload)?;
    let key = row.row_id.clone().into();
    validate_canonical_project_text(payload, &row, &key)?;
    if row.id != RecordId::from(row.row_id.clone())
        || &row.project != project
        || &row.scope != project
    {
        return Err(ProjectStorageError::InvalidFeed);
    }
    row.position()?;
    Ok(row)
}
impl ProjectSnapshot {
    pub async fn has_owner_question(&mut self) -> Result<bool, ProjectStorageError> {
        Ok(sqlx_turso::query_scalar!(r#"SELECT EXISTS(SELECT 1 FROM work_trail_tags t JOIN work_trail_rows r ON r.row_id = t.row_id WHERE t.tag = 'owner-question' AND r.redacted = 0) AS "present!: bool""#).fetch_one(&mut *self.transaction).await?)
    }
    pub async fn holds_project_lead(
        &mut self,
        reader: &Identity,
    ) -> Result<bool, ProjectStorageError> {
        let project = self.project_id.as_str();
        let row = sqlx_turso::query!(r#"SELECT json_extract(r.payload, '$.record.holder.actor') AS "holder!: String", json_quote(json_extract(r.payload, '$.record.ended')) AS "ended!: String" FROM role_assignments a CROSS JOIN record_identities r ON r.kind = a.kind AND r.id = a.assignment_id WHERE a.project_subject = ? AND a.role = 'lead' AND a.active = 1 LIMIT 1"#, project).fetch_optional(&mut *self.transaction).await?;
        let Some(row) = row else {
            return Ok(false);
        };
        let holder: ActorKind = serde_json::from_str(&row.holder)?;
        let ended: Option<RoleEnd> = serde_json::from_str(&row.ended)?;
        Ok(ended.is_none()
            && matches!(holder, ActorKind::Declared(identity) if &identity == reader))
    }
    pub async fn pending_trail_inputs(
        &mut self,
        selection: PendingTrailSelection,
    ) -> Result<PendingTrailInputs, ProjectStorageError> {
        self.scoped_pending_trail_inputs(selection, None).await
    }
    pub async fn pending_card_trail_inputs(
        &mut self,
        subject: &TrailSubject,
        handbacks_by: Option<&Identity>,
    ) -> Result<PendingTrailInputs, ProjectStorageError> {
        self.scoped_pending_trail_inputs(
            PendingTrailSelection::Both,
            Some(PendingCardScope {
                subject,
                handbacks_by,
            }),
        )
        .await
    }
    async fn scoped_pending_trail_inputs(
        &mut self,
        selection: PendingTrailSelection,
        card: Option<PendingCardScope<'_>>,
    ) -> Result<PendingTrailInputs, ProjectStorageError> {
        let mut candidates = std::collections::BTreeMap::new();
        let tags: &[&str] = match selection {
            PendingTrailSelection::Questions => &["owner-question"],
            PendingTrailSelection::Handbacks => &["handback"],
            PendingTrailSelection::Both => &["handback", "owner-question"],
        };
        let include_authority = !matches!(selection, PendingTrailSelection::Questions)
            && card
                .as_ref()
                .is_none_or(|scope| scope.handbacks_by.is_some());
        for tag in tags {
            let mut after = String::new();
            loop {
                let rows: Vec<(String, String)> = if let Some(scope) = &card {
                    let subject = serde_json::to_string(scope.subject)?;
                    let actor = scope.handbacks_by.map(serde_json::to_string).transpose()?;
                    sqlx_turso::query_file!(
                        "src/project_view_pending_card_candidates.sql",
                        subject,
                        tag,
                        after,
                        actor
                    )
                    .fetch_all(&mut *self.transaction)
                    .await?
                    .into_iter()
                    .map(|row| (row.row_id, row.metadata))
                    .collect()
                } else {
                    sqlx_turso::query_file!("src/project_view_pending_candidates.sql", tag, after)
                        .fetch_all(&mut *self.transaction)
                        .await?
                        .into_iter()
                        .map(|row| (row.row_id, row.metadata))
                        .collect()
                };
                let exhausted = rows.len() <= 100;
                for (id, payload) in rows.into_iter().take(100) {
                    after = id;
                    let metadata = decode_metadata(&payload, &self.project_id)?;
                    if metadata.row_id.as_str() != after || !metadata.has_tag(tag) {
                        return Err(ProjectStorageError::InvalidFeed);
                    }
                    candidates.insert(metadata.row_id.clone(), metadata);
                }
                if exhausted {
                    break;
                }
            }
        }
        if candidates.is_empty() {
            return Ok(PendingTrailInputs::default());
        }
        let ids = serde_json::to_string(&candidates.keys().collect::<Vec<_>>())?;
        let project = self.project_id.as_str();
        let mut after = String::new();
        let mut decisions = Vec::new();
        loop {
            let rows = sqlx_turso::query_file!(
                "src/project_view_pending_decisions.sql",
                include_authority,
                project,
                after,
                ids
            )
            .fetch_all(&mut *self.transaction)
            .await?;
            let exhausted = rows.len() <= 100;
            for row in rows.into_iter().take(100) {
                after = row.row_id;
                let metadata = decode_metadata(&row.metadata, &self.project_id)?;
                if metadata.row_id.as_str() != after || !metadata.has_tag("decision") {
                    return Err(ProjectStorageError::InvalidFeed);
                }
                let authority = row
                    .authority
                    .map(|payload| serde_json::from_str::<DecisionAuthority>(&payload))
                    .transpose()?;
                let authority = authority
                    .map(|authority| {
                        if authority.role != AuthorityRole::Lead
                            || authority.subject != RoleSubject::Project(self.project_id.clone())
                        {
                            return Err(ProjectStorageError::InvalidFeed);
                        }
                        let ActorKind::Declared(holder) = authority.holder else {
                            return Err(ProjectStorageError::InvalidFeed);
                        };
                        Ok(HistoricalLeadMetadata {
                            holder,
                            ended: authority.ended,
                        })
                    })
                    .transpose()?;
                decisions.push(TrailDecisionMetadata {
                    row: metadata,
                    authority,
                });
            }
            if exhausted {
                break;
            }
        }
        Ok(PendingTrailInputs {
            candidates: candidates.into_values().collect(),
            decisions,
        })
    }
}
