//! Legacy event order becomes the live record model; no clock is consulted.
mod participant_history;
mod participant_intervals;
use participant_intervals::PendingParticipant;
mod reader_translation;
use crate::{DerivedImportKeys, UpgradeError, import_error::invalid, legacy_rows::*};
use fleet_admission::ImportProvenance;
use message_board::*;
use message_board_storage::{ImportRecord, ImportWatch, ProjectImportBatch};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct TransformedProject {
    pub batch: ProjectImportBatch,
    pub mapping: BTreeMap<u64, FeedCursor>,
    pub resolutions: u64,
    pub post_count: u64,
}

struct ProjectBuilder<'a> {
    source: &'a LegacyStore,
    project: &'a LegacyProject,
    scope: RecordScope,
    provenance: &'a ImportProvenance,
    keys: DerivedImportKeys,
    rows: Vec<ImportRecord>,
    mapping: BTreeMap<u64, FeedCursor>,
    threads: BTreeMap<ThreadId, Thread>,
    participants: BTreeMap<(ThreadId, Identity), PendingParticipant>,
}

pub(crate) fn transform(
    source: &LegacyStore,
    project: &LegacyProject,
    provenance: &ImportProvenance,
    owner: &HumanId,
) -> Result<TransformedProject, UpgradeError> {
    let board = source
        .boards
        .iter()
        .find(|board| board.board_id == project.board_id)
        .ok_or_else(|| invalid("board_id", "parent missing"))?;
    let mut builder = ProjectBuilder {
        source,
        project,
        scope: RecordScope {
            workspace: board.project_id.clone(),
            board: Some(project.board_id.clone()),
            project: Some(project.topic_id.clone()),
        },
        provenance,
        keys: DerivedImportKeys::new(&project.topic_id),
        rows: Vec::new(),
        mapping: BTreeMap::new(),
        threads: BTreeMap::new(),
        participants: BTreeMap::new(),
    };
    let state = ProjectState {
        envelope: builder.envelope(
            project.topic_id.clone().into(),
            RecordedActor::Human {
                human_id: owner.clone(),
            },
        )?,
        project_id: project.topic_id.clone(),
        description: project.description.clone(),
        status: None,
        pinned_definition: None,
        revision: SubjectRevision::INITIAL,
        closed_at: None,
    };
    builder.push(Record::Project(state), Vec::new())?;
    let mut consumed = BTreeSet::new();
    let mut imported_posts = BTreeSet::new();
    let mut resolutions = 0;
    let mut post_count = 0;
    for activity in source
        .activity
        .iter()
        .filter(|row| row.topic_id == project.topic_id)
    {
        if consumed.contains(&activity.activity_sequence) {
            continue;
        }
        match activity.kind {
            ActivityKind::MainMessageCreated | ActivityKind::ThreadMessageCreated => {
                let post_id = activity
                    .message_id
                    .as_ref()
                    .ok_or_else(|| invalid("message_id", "message activity has no post"))?;
                let post = source
                    .posts
                    .get(post_id)
                    .ok_or_else(|| invalid("message_id", "post missing"))?;
                if !imported_posts.insert(post_id.clone()) {
                    return Err(invalid(
                        "post.coverage",
                        "post has repeated creation activity",
                    ));
                }
                let actor = builder.actor(&post.actor_key)?;
                if activity.actor_key != post.actor_key
                    || post.topic_id != project.topic_id
                    || post.board_id != project.board_id
                {
                    return Err(invalid("post", "activity disagrees with post"));
                }
                let text = post
                    .text
                    .chars()
                    .map(|character| {
                        if character <= '\u{001f}' && !matches!(character, '\n' | '\t') {
                            format!("\\u{:04x}", u32::from(character))
                        } else {
                            character.to_string()
                        }
                    })
                    .collect::<String>();
                let role = builder.posted_role(post, activity, &actor)?;
                let creation_join = if post.root_id.is_none() {
                    source
                        .activity
                        .iter()
                        .find(|event| event.activity_sequence > activity.activity_sequence)
                        .filter(|event| {
                            matches!(event.kind, ActivityKind::ParticipantJoined)
                                && event.root_id.as_ref() == Some(&ThreadId::from(post_id.clone()))
                                && event.participant_key.as_deref() == Some(post.actor_key.as_str())
                                && event.topic_id == project.topic_id
                        })
                } else {
                    None
                };
                if matches!(activity.kind, ActivityKind::MainMessageCreated) {
                    if post.root_id.is_some() || activity.root_id.is_some() {
                        return Err(invalid("root_id", "main post has root"));
                    }
                    let thread_id = ThreadId::from(post_id.clone());
                    let line = text
                        .split('\n')
                        .next()
                        .unwrap_or_default()
                        .trim()
                        .chars()
                        .take(120)
                        .collect::<String>();
                    let line = if line.is_empty() {
                        format!("Imported thread {}", thread_id.as_str())
                    } else {
                        line
                    };
                    let mut thread = Thread {
                        envelope: builder.envelope(
                            thread_id.clone().into(),
                            RecordedActor::declared(actor.clone()),
                        )?,
                        thread_id: thread_id.clone(),
                        about: ThreadSubject::Project {
                            line: SubjectLine::try_from(line)
                                .map_err(|error| invalid("subject", error))?,
                        },
                        imported: true,
                        state: ThreadState::Open,
                        orchestrator: None,
                        implementer: None,
                    };
                    if let Some(join) = creation_join {
                        let position = cursor(builder.next_cursor()?.position.get() + 1)?;
                        match join.participant_role {
                            Some(ParticipantRole::Orchestrator) => {
                                thread.orchestrator = Some(OrchestratorHolder {
                                    identity: actor.clone(),
                                    last_seen_position: position,
                                })
                            }
                            Some(ParticipantRole::Implementer) => {
                                thread.implementer = Some(ImplementerHolder {
                                    identity: actor.clone(),
                                    last_seen_position: position,
                                })
                            }
                            _ => {}
                        }
                    }
                    builder.push(Record::Thread(thread.clone()), Vec::new())?;
                    builder.threads.insert(thread_id, thread);
                } else if activity.root_id != post.root_id || post.root_id.is_none() {
                    return Err(invalid("root_id", "reply activity disagrees with post"));
                }
                let acting_for = post
                    .acting_for_key
                    .as_ref()
                    .map(|key| builder.actor(key))
                    .transpose()?
                    .map(|identity| match identity {
                        Identity::Human { human_id } => Ok(ActingForIdentity::Human { human_id }),
                        Identity::Session { .. } => Err(invalid("acting_for", "not a human")),
                    })
                    .transpose()?;
                let envelope = builder.envelope(
                    post_id.clone().into(),
                    RecordedActor::declared(actor.clone()),
                )?;
                let row = Post {
                    position: envelope.position.clone(),
                    envelope,
                    post_id: post_id.clone(),
                    board_id: post.board_id.clone(),
                    project_id: post.topic_id.clone(),
                    placement: post.root_id.clone().map_or_else(
                        || Placement::Project {
                            project_id: project.topic_id.clone(),
                        },
                        |thread_id| Placement::Thread { thread_id },
                    ),
                    actor: actor.clone(),
                    acting_for,
                    text: PostText::try_from(text).map_err(|error| invalid("text", error))?,
                    references: PostReferences::try_from(
                        source.references.get(post_id).cloned().unwrap_or_default(),
                    )
                    .map_err(|error| invalid("references", error))?,
                    posted_as_role: role,
                };
                let cursor = builder.push(Record::Post(row), Vec::new())?;
                let thread = post
                    .root_id
                    .clone()
                    .unwrap_or_else(|| ThreadId::from(post_id.clone()));
                if let Some(join) = creation_join {
                    let sequence = join.activity_sequence;
                    let changes = builder.join(
                        &thread,
                        &actor,
                        builder.role_for_join(join)?,
                        cursor,
                        None,
                        ParticipantClosedReason::Rejoined,
                    )?;
                    builder
                        .rows
                        .last_mut()
                        .ok_or_else(|| invalid("post", "missing import row"))?
                        .participants = changes;
                    builder.mapping.insert(sequence, cursor);
                    consumed.insert(sequence);
                } else if let Some(participant) = builder.participants.get_mut(&(thread, actor)) {
                    let participant = participant.observe_post(cursor);
                    if let Some(participant) = participant {
                        builder
                            .rows
                            .last_mut()
                            .ok_or_else(|| invalid("post", "missing import row"))?
                            .participants
                            .push((
                                post.root_id
                                    .clone()
                                    .ok_or_else(|| invalid("root_id", "missing"))?,
                                participant,
                            ));
                    }
                }
                builder.mapping.insert(activity.activity_sequence, cursor);
                post_count += 1;
            }
            ActivityKind::ParticipantJoined
            | ActivityKind::OrchestratorReplaced
            | ActivityKind::ImplementerReplaced
            | ActivityKind::ParticipantLeft
            | ActivityKind::ThreadResolved
            | ActivityKind::ThreadUnresolved => {
                let thread_id = activity
                    .root_id
                    .clone()
                    .ok_or_else(|| invalid("root_id", "event has no thread"))?;
                let cursor = builder.next_cursor()?;
                let mut changes = Vec::new();
                if let Some(replaced) = &activity.replaced_participant_key {
                    let identity = builder.actor(replaced)?;
                    let successor = builder.actor(
                        activity
                            .participant_key
                            .as_deref()
                            .ok_or_else(|| invalid("participant_key", "replacement missing"))?,
                    )?;
                    changes.push((
                        thread_id.clone(),
                        builder.close_interval(
                            &thread_id,
                            &identity,
                            cursor,
                            ParticipantClosedReason::Replaced,
                            Some(successor),
                        )?,
                    ));
                }
                match activity.kind {
                    ActivityKind::ParticipantJoined
                    | ActivityKind::OrchestratorReplaced
                    | ActivityKind::ImplementerReplaced => {
                        let identity =
                            builder.actor(activity.participant_key.as_deref().ok_or_else(
                                || invalid("participant_key", "join missing actor"),
                            )?)?;
                        changes.extend(builder.join(
                            &thread_id,
                            &identity,
                            builder.role_for_join(activity)?,
                            cursor,
                            None,
                            if matches!(activity.kind, ActivityKind::OrchestratorReplaced)
                                && activity.participant_key.as_deref()
                                    != Some(activity.actor_key.as_str())
                            {
                                ParticipantClosedReason::Promoted
                            } else {
                                ParticipantClosedReason::Rejoined
                            },
                        )?);
                    }
                    ActivityKind::ParticipantLeft => {
                        let identity =
                            builder.actor(activity.participant_key.as_deref().ok_or_else(
                                || invalid("participant_key", "leave missing actor"),
                            )?)?;
                        changes.push((
                            thread_id.clone(),
                            builder.close_interval(
                                &thread_id,
                                &identity,
                                cursor,
                                ParticipantClosedReason::Left,
                                None,
                            )?,
                        ));
                    }
                    ActivityKind::ThreadResolved => {
                        let identities = builder
                            .participants
                            .keys()
                            .filter(|(thread, _)| *thread == thread_id)
                            .map(|(_, identity)| identity.clone())
                            .collect::<Vec<_>>();
                        for identity in identities {
                            changes.push((
                                thread_id.clone(),
                                builder.close_interval(
                                    &thread_id,
                                    &identity,
                                    cursor,
                                    ParticipantClosedReason::Resolved,
                                    None,
                                )?,
                            ));
                        }
                        builder
                            .threads
                            .get_mut(&thread_id)
                            .ok_or_else(|| invalid("thread", "missing"))?
                            .state = ThreadState::Resolved;
                        resolutions += 1;
                    }
                    ActivityKind::ThreadUnresolved => {
                        builder
                            .threads
                            .get_mut(&thread_id)
                            .ok_or_else(|| invalid("thread", "missing"))?
                            .state = ThreadState::Open
                    }
                    ActivityKind::MainMessageCreated | ActivityKind::ThreadMessageCreated => {
                        return Err(invalid("kind", "unexpected message branch"));
                    }
                }
                let mut thread = builder
                    .threads
                    .get(&thread_id)
                    .cloned()
                    .ok_or_else(|| invalid("thread", "missing"))?;
                thread.envelope = builder.envelope(
                    thread_id.clone().into(),
                    RecordedActor::declared(builder.actor(&activity.actor_key)?),
                )?;
                builder.update_holders(&mut thread);
                builder.threads.insert(thread_id, thread.clone());
                let cursor = builder.push(Record::Thread(thread), changes)?;
                builder.mapping.insert(activity.activity_sequence, cursor);
            }
        }
    }
    if source
        .posts
        .values()
        .any(|post| post.topic_id == project.topic_id && !imported_posts.contains(&post.message_id))
    {
        return Err(invalid(
            "post.coverage",
            "source post has no creation activity",
        ));
    }
    for (id, thread) in &builder.threads {
        if source.threads.get(id) != Some(&thread.state) {
            return Err(invalid(
                "thread.state",
                format!("history disagrees with current thread {}", id.as_str()),
            ));
        }
    }
    builder.finish_participants()?;
    let (read_marks, watches) = builder.finish_readers()?;
    let batch = ProjectImportBatch {
        records: std::mem::take(&mut builder.rows),
        read_marks,
        watches,
    };
    Ok(TransformedProject {
        batch,
        mapping: builder.mapping,
        resolutions,
        post_count,
    })
}

impl ProjectBuilder<'_> {
    fn actor(&self, key: &str) -> Result<Identity, UpgradeError> {
        self.source
            .actors
            .get(key)
            .cloned()
            .ok_or_else(|| invalid("actor_key", format!("unknown actor {key}")))
    }
    fn next_cursor(&self) -> Result<FeedCursor, UpgradeError> {
        cursor(u64::try_from(self.rows.len()).map_err(|error| invalid("position", error))? + 1)
    }
    fn envelope(&self, id: RecordId, actor: RecordedActor) -> Result<RecordEnvelope, UpgradeError> {
        Ok(RecordEnvelope {
            id,
            scope: self.scope.clone(),
            actor,
            request: None,
            position: RecordPlacement::Project(ProjectFeedEntry {
                cursor: self.next_cursor()?,
                origin: ActorOrigin {
                    installation: self.provenance.installation.clone(),
                    machine: self.provenance.machine.clone(),
                },
            }),
            time: RecordTime::LegacyUnknown,
        })
    }
    fn push(
        &mut self,
        record: Record,
        participants: Vec<(ThreadId, Participant)>,
    ) -> Result<FeedCursor, UpgradeError> {
        let cursor = self.next_cursor()?;
        if record.envelope().position.project_cursor() != Some(cursor) {
            return Err(invalid("position", "transform is not dense"));
        }
        self.rows.push(ImportRecord {
            record,
            participants,
        });
        Ok(cursor)
    }
    fn cutoff(&self, old: u64) -> Result<FeedCursor, UpgradeError> {
        Ok(self
            .mapping
            .range(..=old)
            .next_back()
            .map_or(cursor(0)?, |(_, cursor)| *cursor))
    }
    fn belongs_to_thread(&self, thread: &str) -> bool {
        self.threads.keys().any(|id| id.as_str() == thread)
    }
    fn posted_role(
        &self,
        post: &LegacyPost,
        activity: &LegacyActivity,
        actor: &Identity,
    ) -> Result<Option<ParticipantRole>, UpgradeError> {
        let Some(sequence) = post.posted_from_activity else {
            return Ok(None);
        };
        let event = self
            .source
            .activity
            .iter()
            .find(|event| event.activity_sequence == sequence)
            .ok_or_else(|| invalid("posted_from_activity", "event missing"))?;
        let root = post
            .root_id
            .clone()
            .unwrap_or_else(|| ThreadId::from(post.message_id.clone()));
        if !matches!(actor, Identity::Session { .. })
            || event.participant_key.as_deref() != Some(post.actor_key.as_str())
            || event.root_id.as_ref() != Some(&root)
            || event.topic_id != post.topic_id
            || (post.root_id.is_some() && sequence >= activity.activity_sequence)
            || (post.root_id.is_none() && !matches!(event.kind, ActivityKind::ParticipantJoined))
        {
            return Err(invalid(
                "posted_from_activity",
                "legacy attribution validation failed",
            ));
        }
        event
            .participant_role
            .map(Some)
            .ok_or_else(|| invalid("participant_role", "attribution missing role"))
    }
}

pub(crate) fn cursor(position: u64) -> Result<FeedCursor, UpgradeError> {
    Ok(FeedCursor {
        epoch: EpochNumber::INITIAL,
        position: FeedPosition::try_from(position).map_err(|error| invalid("position", error))?,
    })
}
