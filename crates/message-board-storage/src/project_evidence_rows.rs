//! Evidence and trail records decode typed rows; only redaction effects live solely in the feed.
use crate::ProjectStorageError;
use crate::project_record_codec::{
    stored_bool, stored_counter, stored_json, stored_project_row, stored_text, stored_timestamp,
};
use message_board::*;
use sqlx::Executor;
use sqlx_turso::Turso;
use std::collections::BTreeMap;

stored_project_row!(StoredFact, FactBody { fact: String, links: String, producer_time: String, anchor_epoch: Option<i64>, anchor_position: Option<i64>, late: i64 });
stored_project_row!(StoredPlanRevision, PlanRevisionBody { content_hash: String, document: String, superseded_by: Option<String>, revision: i64 });
stored_project_row!(
    StoredTrail,
    TrailBody {
        subject: String,
        text: String,
        redacted: i64
    }
);
stored_project_row!(StoredRedaction, RedactionBody { content: String });

pub(crate) struct StoredTrailTag {
    tag_id: String,
    place: i64,
    tag: String,
}
pub(crate) struct StoredTrailLink {
    link_id: String,
    place: i64,
    target: String,
}
pub(crate) enum StoredEvidenceRecord {
    Fact(StoredFact),
    PlanRevision(StoredPlanRevision),
    Trail {
        row: StoredTrail,
        tags: Vec<StoredTrailTag>,
        links: Vec<StoredTrailLink>,
    },
    Redaction(StoredRedaction),
}
impl StoredEvidenceRecord {
    pub(crate) fn reference(&self) -> Result<RecordRef, ProjectStorageError> {
        match self {
            Self::Fact(row) => row.reference(RecordKind::Fact),
            Self::PlanRevision(row) => row.reference(RecordKind::PlanRevision),
            Self::Trail { row, .. } => row.reference(RecordKind::WorkTrailRow),
            Self::Redaction(row) => row.reference(RecordKind::TrailRedaction),
        }
    }
    pub(crate) fn decode(self, project: &ProjectId) -> Result<Record, ProjectStorageError> {
        let reference = self.reference()?;
        let invalid = |field| ProjectStorageError::InvalidRecord {
            record: reference.clone(),
            field,
        };
        Ok(match self {
            Self::Fact(row) => {
                let (raw, body) = row.into_parts();
                let anchor = match (body.anchor_epoch, body.anchor_position) {
                    (None, None) => None,
                    (Some(epoch), Some(position)) => Some(FeedCursor {
                        epoch: stored_counter(epoch, &reference, "anchor")?,
                        position: stored_counter(position, &reference, "anchor")?,
                    }),
                    _ => return Err(invalid("anchor")),
                };
                Record::Fact(Fact {
                    envelope: raw.decode(&reference, project)?,
                    fact_id: FactId::try_from(reference.id.as_str().to_owned())
                        .map_err(|_| invalid("id"))?,
                    kind: stored_json(&body.fact, &reference, "kind")?,
                    links: stored_json(&body.links, &reference, "links")?,
                    producer_time: stored_timestamp(
                        body.producer_time,
                        &reference,
                        "producerTime",
                    )?,
                    anchor,
                    late: stored_bool(body.late, &reference, "late")?,
                })
            }
            Self::PlanRevision(row) => {
                let (raw, body) = row.into_parts();
                Record::PlanRevision(PlanRevision {
                    envelope: raw.decode(&reference, project)?,
                    hash: stored_text(
                        body.content_hash,
                        &reference,
                        "hash",
                        PlanRevisionHash::as_str,
                    )?,
                    document: stored_json(&body.document, &reference, "document")?,
                    superseded_by: body
                        .superseded_by
                        .map(|value| {
                            stored_text(value, &reference, "supersededBy", PlanRevisionHash::as_str)
                        })
                        .transpose()?,
                    revision: stored_counter(body.revision, &reference, "revision")?,
                })
            }
            Self::Trail { row, tags, links } => {
                let (raw, body) = row.into_parts();
                let mut decoded_tags = Vec::with_capacity(tags.len());
                for (place, tag) in tags.into_iter().enumerate() {
                    RecordId::try_from(tag.tag_id).map_err(|_| invalid("tags.id"))?;
                    if usize::try_from(tag.place).ok() != Some(place) {
                        return Err(invalid("tags.place"));
                    }
                    decoded_tags.push(stored_text(tag.tag, &reference, "tags", TrailTag::as_str)?);
                }
                let mut decoded_links = Vec::with_capacity(links.len());
                for (place, link) in links.into_iter().enumerate() {
                    RecordId::try_from(link.link_id).map_err(|_| invalid("links.id"))?;
                    if usize::try_from(link.place).ok() != Some(place) {
                        return Err(invalid("links.place"));
                    }
                    decoded_links.push(stored_json(&link.target, &reference, "links")?);
                }
                Record::WorkTrailRow(WorkTrailRow {
                    envelope: raw.decode(&reference, project)?,
                    row_id: WorkTrailRowId::try_from(reference.id.as_str().to_owned())
                        .map_err(|_| invalid("id"))?,
                    project_id: project.clone(),
                    subject: stored_json(&body.subject, &reference, "subject")?,
                    text: stored_text(body.text, &reference, "text", TrailText::as_str)?,
                    tags: decoded_tags,
                    links: decoded_links,
                    redacted: stored_bool(body.redacted, &reference, "redacted")?,
                })
            }
            Self::Redaction(row) => {
                let (raw, body) = row.into_parts();
                let envelope = raw.decode(&reference, project)?;
                let FeedContent::Shown(record) = stored_json(&body.content, &reference, "content")?
                else {
                    return Err(invalid("content"));
                };
                let Record::TrailRedaction(row) = *record else {
                    return Err(invalid("kind"));
                };
                if row.envelope != envelope {
                    return Err(invalid("feedEnvelope"));
                }
                Record::TrailRedaction(row)
            }
        })
    }
}

pub(crate) async fn evidence_records<TScope>(
    transaction: &mut TScope,
    project: &ProjectId,
    kind: RecordKind,
    keys: &[&str],
) -> Result<Vec<StoredEvidenceRecord>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    let project = project.as_str();
    let keys = serde_json::to_string(keys)?;
    Ok(match kind {
        RecordKind::Fact => {
            sqlx_turso::query_file_as!(StoredFact, "src/project_fact_batch.sql", project, keys)
                .fetch_all(&mut *transaction)
                .await?
                .into_iter()
                .map(StoredEvidenceRecord::Fact)
                .collect()
        }
        RecordKind::PlanRevision => sqlx_turso::query_file_as!(
            StoredPlanRevision,
            "src/project_plan_revision_batch.sql",
            project,
            keys
        )
        .fetch_all(&mut *transaction)
        .await?
        .into_iter()
        .map(StoredEvidenceRecord::PlanRevision)
        .collect(),
        RecordKind::TrailRedaction => sqlx_turso::query_file_as!(
            StoredRedaction,
            "src/project_redaction_batch.sql",
            project,
            keys
        )
        .fetch_all(&mut *transaction)
        .await?
        .into_iter()
        .map(StoredEvidenceRecord::Redaction)
        .collect(),
        RecordKind::WorkTrailRow => {
            let rows = sqlx_turso::query_file_as!(
                StoredTrail,
                "src/project_trail_batch.sql",
                project,
                keys
            )
            .fetch_all(&mut *transaction)
            .await?;
            let tags = sqlx_turso::query!(r#"SELECT tag_id AS "tag_id!: String", row_id AS "row_id!: String", place AS "place!: i64", tag AS "tag!: String" FROM work_trail_tags WHERE row_id IN (SELECT value FROM json_each(?)) ORDER BY row_id, place"#, keys).fetch_all(&mut *transaction).await?;
            let links = sqlx_turso::query!(r#"SELECT link_id AS "link_id!: String", row_id AS "row_id!: String", place AS "place!: i64", target AS "target!: String" FROM work_trail_links WHERE row_id IN (SELECT value FROM json_each(?)) ORDER BY row_id, place"#, keys).fetch_all(&mut *transaction).await?;
            let mut by_tag: BTreeMap<String, Vec<StoredTrailTag>> = BTreeMap::new();
            for row in tags {
                by_tag.entry(row.row_id).or_default().push(StoredTrailTag {
                    tag_id: row.tag_id,
                    place: row.place,
                    tag: row.tag,
                });
            }
            let mut by_link: BTreeMap<String, Vec<StoredTrailLink>> = BTreeMap::new();
            for row in links {
                by_link
                    .entry(row.row_id)
                    .or_default()
                    .push(StoredTrailLink {
                        link_id: row.link_id,
                        place: row.place,
                        target: row.target,
                    });
            }
            rows.into_iter()
                .map(|row| {
                    let tags = by_tag.remove(&row.record_id).unwrap_or_default();
                    let links = by_link.remove(&row.record_id).unwrap_or_default();
                    StoredEvidenceRecord::Trail { row, tags, links }
                })
                .collect()
        }
        _ => return Err(ProjectStorageError::InvalidFeed),
    })
}
