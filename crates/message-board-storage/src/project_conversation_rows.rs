//! Conversation bodies and their ordered references remain page-local and lazily decoded.
use crate::ProjectStorageError;
use crate::project_actor_rows::decode_actor;
use crate::project_record_codec::{
    stored_bool, stored_counter, stored_json, stored_project_row, stored_text,
};
use message_board::{
    FeedCursor, ImplementerHolder, OrchestratorHolder, Placement, Post, PostId, PostReferences,
    PostText, ProjectId, Record, RecordId, RecordKind, RecordRef, Thread, ThreadId, ThreadRefiled,
};
use sqlx::Executor;
use sqlx_turso::Turso;
use std::collections::BTreeMap;

stored_project_row!(
    StoredThread,
    ThreadBody {
        about: String,
        imported: i64,
        state: String
    }
);
stored_project_row!(StoredPost, PostBody { thread_id: String, placement_kind: String, acting_for: Option<String>, text: String, posted_as_role: Option<String> });
stored_project_row!(
    StoredRefiling,
    RefilingBody {
        thread_id: String,
        from_subject: String,
        to_subject: String
    }
);

pub(crate) struct StoredPostReference {
    reference_id: String,
    place: i64,
    target: String,
}
pub(crate) struct StoredThreadSeat {
    participation_id: String,
    reader_actor: String,
    reader_identity: Option<String>,
    role: String,
    joined_epoch: i64,
    joined_position: i64,
    last_seen_epoch: i64,
    last_seen_position: i64,
    closed_position: Option<i64>,
    closed_reason: Option<String>,
    replaced_by: Option<String>,
}

pub(crate) enum StoredConversationRecord {
    Thread {
        row: StoredThread,
        seats: Vec<StoredThreadSeat>,
    },
    Post {
        row: StoredPost,
        references: Vec<StoredPostReference>,
    },
    Refiling(StoredRefiling),
}

impl StoredConversationRecord {
    pub(crate) fn reference(&self) -> Result<RecordRef, ProjectStorageError> {
        match self {
            Self::Thread { row, .. } => row.reference(RecordKind::Thread),
            Self::Post { row, .. } => row.reference(RecordKind::Post),
            Self::Refiling(row) => row.reference(RecordKind::ThreadRefiled),
        }
    }
    pub(crate) fn decode(self, project: &ProjectId) -> Result<Record, ProjectStorageError> {
        let reference = self.reference()?;
        let invalid = |field| ProjectStorageError::InvalidRecord {
            record: reference.clone(),
            field,
        };
        Ok(match self {
            Self::Thread { row, seats } => {
                let (raw, body) = row.into_parts();
                let mut orchestrator = None;
                let mut implementer = None;
                for seat in seats {
                    RecordId::try_from(seat.participation_id)
                        .map_err(|_| invalid("participant.id"))?;
                    let identity = decode_actor(
                        &seat.reader_actor,
                        &seat
                            .reader_identity
                            .ok_or_else(|| invalid("participant.identity"))?,
                    )?
                    .declared_identity()
                    .ok_or_else(|| invalid("participant.identity"))?;
                    let joined = FeedCursor {
                        epoch: stored_counter(seat.joined_epoch, &reference, "participant.joined")?,
                        position: stored_counter(
                            seat.joined_position,
                            &reference,
                            "participant.joined",
                        )?,
                    };
                    let last_seen_position = FeedCursor {
                        epoch: stored_counter(
                            seat.last_seen_epoch,
                            &reference,
                            "participant.lastSeen",
                        )?,
                        position: stored_counter(
                            seat.last_seen_position,
                            &reference,
                            "participant.lastSeen",
                        )?,
                    };
                    if seat.closed_position.is_some()
                        || seat.closed_reason.is_some()
                        || seat.replaced_by.is_some()
                        || last_seen_position.position < joined.position
                        || last_seen_position.epoch < joined.epoch
                    {
                        return Err(invalid("participant"));
                    }
                    match seat.role.as_str() {
                        "orchestrator" if orchestrator.is_none() => {
                            orchestrator = Some(OrchestratorHolder {
                                identity,
                                last_seen_position,
                            })
                        }
                        "implementer" if implementer.is_none() => {
                            implementer = Some(ImplementerHolder {
                                identity,
                                last_seen_position,
                            })
                        }
                        _ => return Err(invalid("participant.role")),
                    }
                }
                Record::Thread(Thread {
                    envelope: raw.decode(&reference, project)?,
                    thread_id: ThreadId::try_from(reference.id.as_str().to_owned())
                        .map_err(|_| invalid("id"))?,
                    about: stored_json(&body.about, &reference, "about")?,
                    imported: stored_bool(body.imported, &reference, "imported")?,
                    state: stored_json(&body.state, &reference, "state")?,
                    orchestrator,
                    implementer,
                })
            }
            Self::Post { row, references } => {
                let (raw, body) = row.into_parts();
                let envelope = raw.decode(&reference, project)?;
                let post_id = PostId::try_from(reference.id.as_str().to_owned())
                    .map_err(|_| invalid("id"))?;
                let thread = stored_text(body.thread_id, &reference, "threadId", ThreadId::as_str)?;
                let placement = match body.placement_kind.as_str() {
                    "project" if thread.as_str() == post_id.as_str() => Placement::Project {
                        project_id: project.clone(),
                    },
                    "thread" => Placement::Thread { thread_id: thread },
                    _ => return Err(invalid("placement")),
                };
                let mut targets = Vec::with_capacity(references.len());
                for (place, stored) in references.into_iter().enumerate() {
                    RecordId::try_from(stored.reference_id)
                        .map_err(|_| invalid("references.id"))?;
                    if usize::try_from(stored.place).ok() != Some(place) {
                        return Err(invalid("references.place"));
                    }
                    targets.push(stored_json(
                        &stored.target,
                        &reference,
                        "references.target",
                    )?);
                }
                Record::Post(Post {
                    post_id,
                    board_id: envelope
                        .scope
                        .board
                        .clone()
                        .ok_or_else(|| invalid("board"))?,
                    project_id: project.clone(),
                    position: envelope.position.clone(),
                    actor: envelope
                        .actor
                        .declared_identity()
                        .ok_or_else(|| invalid("actor"))?,
                    envelope,
                    placement,
                    acting_for: body
                        .acting_for
                        .map(|value| stored_json(&value, &reference, "actingFor"))
                        .transpose()?,
                    text: stored_text(body.text, &reference, "text", PostText::as_str)?,
                    references: PostReferences::try_from(targets)
                        .map_err(|_| invalid("references"))?,
                    posted_as_role: body
                        .posted_as_role
                        .map(|value| stored_json(&value, &reference, "postedAsRole"))
                        .transpose()?,
                })
            }
            Self::Refiling(row) => {
                let (raw, body) = row.into_parts();
                Record::ThreadRefiled(ThreadRefiled {
                    envelope: raw.decode(&reference, project)?,
                    thread: stored_text(body.thread_id, &reference, "thread", ThreadId::as_str)?,
                    from: stored_json(&body.from_subject, &reference, "from")?,
                    to: stored_json(&body.to_subject, &reference, "to")?,
                })
            }
        })
    }
}

pub(crate) async fn conversation_records<TScope>(
    transaction: &mut TScope,
    project: &ProjectId,
    kind: RecordKind,
    keys: &[&str],
) -> Result<Vec<StoredConversationRecord>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    let project = project.as_str();
    let keys = serde_json::to_string(keys)?;
    Ok(match kind {
        RecordKind::Thread => {
            let rows = sqlx_turso::query_file_as!(
                StoredThread,
                "src/project_thread_batch.sql",
                project,
                keys
            )
            .fetch_all(&mut *transaction)
            .await?;
            let seats = sqlx_turso::query!(r#"SELECT p.thread_id AS "thread_id!: String", p.participation_id AS "participation_id!: String", p.actor_id AS "reader_actor!: String", a.canonical_identity AS "reader_identity?: String", p.role AS "role!: String", p.joined_epoch AS "joined_epoch!: i64", p.joined_position AS "joined_position!: i64", p.last_seen_epoch AS "last_seen_epoch!: i64", p.last_seen_position AS "last_seen_position!: i64", p.closed_position AS "closed_position?: i64", p.closed_reason AS "closed_reason?: String", p.replaced_by AS "replaced_by?: String" FROM thread_participation p LEFT JOIN actors a ON a.actor_id=p.actor_id WHERE p.thread_id IN (SELECT value FROM json_each(?)) AND p.closed_epoch IS NULL AND p.role IN ('orchestrator', 'implementer') ORDER BY p.thread_id, p.role, p.participation_id"#, keys).fetch_all(&mut *transaction).await?;
            let mut by_thread: BTreeMap<String, Vec<StoredThreadSeat>> = BTreeMap::new();
            for seat in seats {
                by_thread
                    .entry(seat.thread_id)
                    .or_default()
                    .push(StoredThreadSeat {
                        participation_id: seat.participation_id,
                        reader_actor: seat.reader_actor,
                        reader_identity: seat.reader_identity,
                        role: seat.role,
                        joined_epoch: seat.joined_epoch,
                        joined_position: seat.joined_position,
                        last_seen_epoch: seat.last_seen_epoch,
                        last_seen_position: seat.last_seen_position,
                        closed_position: seat.closed_position,
                        closed_reason: seat.closed_reason,
                        replaced_by: seat.replaced_by,
                    });
            }
            rows.into_iter()
                .map(|row| {
                    let seats = by_thread.remove(&row.record_id).unwrap_or_default();
                    StoredConversationRecord::Thread { row, seats }
                })
                .collect()
        }
        RecordKind::Post => {
            let rows =
                sqlx_turso::query_file_as!(StoredPost, "src/project_post_batch.sql", project, keys)
                    .fetch_all(&mut *transaction)
                    .await?;
            let references = sqlx_turso::query!(r#"SELECT reference_id AS "reference_id!: String", source_post AS "source_post!: String", place AS "place!: i64", target AS "target!: String" FROM post_references WHERE source_post IN (SELECT value FROM json_each(?)) ORDER BY source_post, place"#, keys).fetch_all(&mut *transaction).await?;
            let mut by_post: BTreeMap<String, Vec<StoredPostReference>> = BTreeMap::new();
            for row in references {
                by_post
                    .entry(row.source_post)
                    .or_default()
                    .push(StoredPostReference {
                        reference_id: row.reference_id,
                        place: row.place,
                        target: row.target,
                    });
            }
            rows.into_iter()
                .map(|row| {
                    let references = by_post.remove(&row.record_id).unwrap_or_default();
                    StoredConversationRecord::Post { row, references }
                })
                .collect()
        }
        RecordKind::ThreadRefiled => sqlx_turso::query_file_as!(
            StoredRefiling,
            "src/project_refiling_batch.sql",
            project,
            keys
        )
        .fetch_all(&mut *transaction)
        .await?
        .into_iter()
        .map(StoredConversationRecord::Refiling)
        .collect(),
        _ => return Err(ProjectStorageError::InvalidFeed),
    })
}
