//! The import identity describes logical content; seed manifests cover physical integrity.
use crate::{UpgradeError, import_error::invalid, legacy_rows::LegacyProject};
use fleet_sync_protocol::ImportDigest;
use message_board::*;
use message_board_storage::ProjectImportBatch;
use serde::Serialize;

pub const IMPORT_DIGEST_BASIS: &str = "board-import-v1: project id/board/title; ordered record ids/scopes/cursors/legacy times/requests/declared actors and bodies, including ProjectState.description; interval mutations; read marks; watch choices and cutoffs. Excludes all record origins and authorless ProjectState owner attribution. Snapshot manifest remains separate seed integrity.";

#[derive(Serialize)]
struct CanonicalImport<'a> {
    version: &'static str,
    project: &'a ProjectId,
    board: &'a BoardId,
    title: &'a ResourceName,
    records: Vec<SemanticEntry<'a>>,
    reader_marks: Vec<SemanticMark<'a>>,
    watches: Vec<SemanticWatch<'a>>,
}
#[derive(Serialize)]
struct SemanticEntry<'a> {
    id: &'a RecordId,
    scope: &'a RecordScope,
    cursor: FeedCursor,
    actor: Option<&'a RecordedActor>,
    request: &'a Option<RequestId>,
    time: RecordTime,
    body: SemanticBody<'a>,
    intervals: &'a [(ThreadId, Participant)],
}
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum SemanticBody<'a> {
    Project {
        project: &'a ProjectId,
        description: &'a Description,
        status: &'a Option<StatusName>,
        pinned: &'a Option<ProcessDefinitionRef>,
        revision: SubjectRevision,
        closed: &'a Option<FeedCursor>,
    },
    Thread {
        thread: &'a ThreadId,
        about: &'a ThreadSubject,
        imported: bool,
        state: ThreadState,
        orchestrator: &'a Option<OrchestratorHolder>,
        implementer: &'a Option<ImplementerHolder>,
    },
    Post {
        post: &'a PostId,
        board: &'a BoardId,
        project: &'a ProjectId,
        placement: &'a Placement,
        actor: &'a Identity,
        acting_for: &'a Option<ActingForIdentity>,
        text: &'a PostText,
        references: &'a PostReferences,
        posted_as_role: Option<ParticipantRole>,
    },
    Membership {
        reader: &'a Identity,
        scope: &'a SubscriptionScope,
        policy: &'a SubscriptionPolicy,
        renewed_at: chrono::DateTime<chrono::Utc>,
        expires_at: chrono::DateTime<chrono::Utc>,
        generation: SubscriptionGeneration,
        activated_at: FeedCursor,
        ended: Option<&'a MembershipEnd>,
    },
}
#[derive(Serialize)]
struct SemanticMark<'a> {
    id: &'a RecordId,
    reader: &'a Identity,
    scope: &'a ReadScope,
    through: FeedCursor,
    time: RecordTime,
}
#[derive(Serialize)]
struct SemanticWatch<'a> {
    reader: &'a Identity,
    thread: &'a Option<ThreadId>,
    starts_after: FeedCursor,
    active: bool,
}

/// Hashes the exact logical projection named by IMPORT_DIGEST_BASIS. In particular,
/// neither a record's accepting origin nor the run's authorless-metadata owner is
/// content. The independent full snapshot manifest still authenticates every page.
pub(crate) fn semantic_import_digest(
    project: &LegacyProject,
    batch: &ProjectImportBatch,
) -> Result<ImportDigest, UpgradeError> {
    let mut records = Vec::new();
    for entry in &batch.records {
        let envelope = entry.record.envelope();
        let (body, actor) = match &entry.record {
            Record::Project(row) => (
                SemanticBody::Project {
                    project: &row.project_id,
                    description: &row.description,
                    status: &row.status,
                    pinned: &row.pinned_definition,
                    revision: row.revision,
                    closed: &row.closed_at,
                },
                None,
            ),
            Record::Thread(row) => (
                SemanticBody::Thread {
                    thread: &row.thread_id,
                    about: &row.about,
                    imported: row.imported,
                    state: row.state,
                    orchestrator: &row.orchestrator,
                    implementer: &row.implementer,
                },
                Some(&envelope.actor),
            ),
            Record::Post(row) => (
                SemanticBody::Post {
                    post: &row.post_id,
                    board: &row.board_id,
                    project: &row.project_id,
                    placement: &row.placement,
                    actor: &row.actor,
                    acting_for: &row.acting_for,
                    text: &row.text,
                    references: &row.references,
                    posted_as_role: row.posted_as_role,
                },
                Some(&envelope.actor),
            ),
            Record::SubscriptionMembership(row) => (
                SemanticBody::Membership {
                    reader: row.reader(),
                    scope: row.scope(),
                    policy: row.policy(),
                    renewed_at: row.renewed_at(),
                    expires_at: row.expires_at(),
                    generation: row.generation(),
                    activated_at: row.activated_at(),
                    ended: row.ended(),
                },
                Some(&envelope.actor),
            ),
            _ => {
                return Err(invalid(
                    "import digest",
                    "record kind is outside legacy import contract",
                ));
            }
        };
        records.push(SemanticEntry {
            id: &envelope.id,
            scope: &envelope.scope,
            cursor: envelope
                .position
                .project_cursor()
                .ok_or_else(|| invalid("import digest", "record cursor missing"))?,
            actor,
            request: &envelope.request,
            time: envelope.time,
            body,
            intervals: &entry.participants,
        });
    }
    let canonical = CanonicalImport {
        version: "board-import-v1",
        project: &project.topic_id,
        board: &project.board_id,
        title: &project.name,
        records,
        reader_marks: batch
            .read_marks
            .iter()
            .map(|mark| SemanticMark {
                id: &mark.envelope.id,
                reader: &mark.reader,
                scope: &mark.scope,
                through: mark.through,
                time: mark.envelope.time,
            })
            .collect(),
        watches: batch
            .watches
            .iter()
            .map(|watch| SemanticWatch {
                reader: &watch.reader,
                thread: &watch.thread,
                starts_after: watch.starts_after,
                active: watch.active,
            })
            .collect(),
    };
    ImportDigest::of_canonical(&canonical)
        .map_err(|error| UpgradeError::Snapshot(error.to_string()))
}
