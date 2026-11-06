//! Use only the normal write and read APIs; a public effect receipt supplies the original cursor on replay.
use crate::PreparedProjectBundle;
use collaboration_client::CollaborationClient;
use message_board::public_replies::{RequestDecision, RequestResult};
use message_board::{
    BoardOperation, ChangeOutcome, CopySource, CurrentWriter, FeedCursor, Identity, PageLimit,
    ProjectRequest, ProjectViewRequest, ProtocolVersion, ReadActor, RecordKind, RecordedActor,
    TrailSubject, WorkTrailReadResult, WorkTrailRowId, WorkTrailSelection, WorkTrailShowRequest,
};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CanaryProof {
    cursor: FeedCursor,
    head_before: FeedCursor,
    head_after: FeedCursor,
    disposition: CanaryDisposition,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
enum CanaryDisposition {
    Created,
    Replayed,
}

type CanaryResult<T> = Result<T, CanaryFailure>;

#[cfg(test)]
#[path = "canary_record_test.rs"]
mod tests;

pub(crate) async fn run_canary(
    client: &CollaborationClient,
    bundle: &PreparedProjectBundle,
) -> CanaryResult<CanaryProof> {
    let manifest = bundle.manifest();
    let service: message_board::ServiceId = String::from(client.identity().service_id.clone())
        .try_into()
        .map_err(|_| {
            CanaryFailure::invalid(CanaryStage::Writer, "invalid declared service identity")
        })?;
    let project = &manifest.source.selected.project;
    let view_request = ProjectViewRequest {
        project_id: project.clone(),
        include_removed: true,
    };
    let before = client.view_work(view_request.clone()).await.map_err(|_| {
        CanaryFailure::invalid(CanaryStage::Writer, "cannot observe the current writer")
    })?;
    check_writer(&before, project, manifest.source.high_water)?;
    let head_before = before.as_of;
    let row_id: WorkTrailRowId = manifest
        .canary_request
        .as_str()
        .to_owned()
        .try_into()
        .map_err(|_| {
            CanaryFailure::invalid(CanaryStage::Bundle, "invalid frozen canary identifier")
        })?;
    let text: message_board::TrailText = format!("C5b cutover canary {}", manifest.run_id)
        .try_into()
        .map_err(|_| CanaryFailure::invalid(CanaryStage::Bundle, "invalid frozen canary text"))?;
    let actor = Identity::Human {
        human_id: manifest.header.declared_owner.clone(),
    };
    let request = ProjectRequest {
        request_id: manifest.canary_request.clone(),
        project_id: project.clone(),
        actor: actor.clone(),
        acting_for: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation: BoardOperation::WorkTrailAdd {
            row_id: row_id.clone(),
            subject: Some(TrailSubject::Project),
            text: text.clone(),
            tags: vec![],
            links: vec![],
        },
    };
    let written =
        client
            .work_trail_add(request.clone())
            .await
            .map_err(|_| CanaryFailure::Unknown {
                stage: CanaryStage::Submission,
            })?;
    let RequestResult::Decided(RequestDecision::Accepted {
        effects,
        change: ChangeOutcome::Changed,
    }) = &written.result
    else {
        return Err(match written.result {
            RequestResult::Unanswered => CanaryFailure::Unknown {
                stage: CanaryStage::Submission,
            },
            _ => CanaryFailure::invalid(
                CanaryStage::Submission,
                "the frozen canary request was not accepted",
            ),
        });
    };
    let [(record, placement)] = effects.entries() else {
        return Err(CanaryFailure::invalid(
            CanaryStage::Submission,
            "canary receipt does not name exactly one effect",
        ));
    };
    let message_board::RecordPlacement::Project(entry) = placement else {
        return Err(CanaryFailure::invalid(
            CanaryStage::Submission,
            "canary effect has no project cursor",
        ));
    };
    if effects.total_count() != 1
        || effects.next().is_some()
        || record.kind != RecordKind::WorkTrailRow
        || record.id.as_str() != row_id.as_str()
        || entry.cursor.epoch.get() != 2
        || entry.origin.installation != service
        || entry.cursor.position <= manifest.source.high_water.position
    {
        return Err(CanaryFailure::invalid(
            CanaryStage::Submission,
            "canary receipt differs from the sealed run",
        ));
    }
    let cursor = entry.cursor;
    let (disposition, expected_head) = if cursor.position > head_before.position {
        // Authority is e2 even when no application row has advanced the inherited e1 seed yet.
        let writer_head = FeedCursor {
            epoch: cursor.epoch,
            position: head_before.position,
        };
        if cursor != next_position(writer_head)? {
            return Err(CanaryFailure::invalid(
                CanaryStage::Head,
                "canary did not append at the observed recovered head plus one",
            ));
        }
        (CanaryDisposition::Created, next_position(cursor)?)
    } else {
        (CanaryDisposition::Replayed, head_before)
    };
    let replayed = client
        .work_trail_add(request)
        .await
        .map_err(|_| CanaryFailure::Unknown {
            stage: CanaryStage::Replay,
        })?;
    if replayed.result != written.result {
        return Err(CanaryFailure::invalid(
            CanaryStage::Replay,
            "the frozen canary replay changed its decision",
        ));
    }
    let preceding = FeedCursor {
        epoch: cursor.epoch,
        position: cursor
            .position
            .get()
            .checked_sub(1)
            .and_then(|position| position.try_into().ok())
            .ok_or(CanaryFailure::invalid(
                CanaryStage::Readback,
                "invalid canary cursor",
            ))?,
    };
    // The predecessor of the first e2 write remains in the sealed e1 history, including on replay.
    let previous = if preceding.position == manifest.source.high_water.position {
        manifest.source.high_water
    } else {
        preceding
    };
    let shown = client
        .work_trail_show(WorkTrailShowRequest {
            reader: ReadActor::Anonymous {},
            project_id: project.clone(),
            selection: WorkTrailSelection::Forward { after: previous },
            limit: PageLimit::try_from(1).map_err(|_| {
                CanaryFailure::invalid(CanaryStage::Readback, "invalid canary page limit")
            })?,
            subject: None,
            tags: vec![],
            include_redacted: true,
        })
        .await
        .map_err(|_| {
            CanaryFailure::invalid(
                CanaryStage::Readback,
                "canary public readback is unavailable",
            )
        })?;
    let WorkTrailReadResult::Forward(page) = shown.value else {
        return Err(CanaryFailure::invalid(
            CanaryStage::Readback,
            "canary cursor was superseded",
        ));
    };
    let [row] = page.rows.as_slice() else {
        return Err(CanaryFailure::invalid(
            CanaryStage::Readback,
            "canary public record is missing",
        ));
    };
    if row.row_id != row_id
        || row.project_id != *project
        || row.text != text
        || row.subject != TrailSubject::Project
        || !row.tags.is_empty()
        || !row.links.is_empty()
        || row.redacted
        || row.envelope.id.as_str() != row_id.as_str()
        || row.envelope.request.as_ref() != Some(&manifest.canary_request)
        || row.envelope.scope != manifest.source.project_scope
        || row.envelope.actor != RecordedActor::declared(actor)
        || row.envelope.position != *placement
    {
        return Err(CanaryFailure::invalid(
            CanaryStage::Readback,
            "canary body, identity or original cursor differs",
        ));
    }
    let after = client.view_work(view_request).await.map_err(|_| {
        CanaryFailure::invalid(CanaryStage::Head, "cannot observe the post-canary writer")
    })?;
    check_writer(&after, project, manifest.source.high_water)?;
    if after.as_of != expected_head {
        return Err(CanaryFailure::invalid(
            CanaryStage::Head,
            "canary replay or concurrent work changed the expected head",
        ));
    }
    Ok(CanaryProof {
        cursor,
        head_before,
        head_after: after.as_of,
        disposition,
    })
}

fn check_writer(
    view: &message_board::WorkView,
    project: &message_board::ProjectId,
    source_head: FeedCursor,
) -> CanaryResult<()> {
    let current_history = matches!(view.as_of.epoch.get(), 1 | 2)
        && (view.as_of == source_head
            || view.as_of.epoch.get() == 2 && view.as_of.position > source_head.position);
    if view.project != *project
        || !matches!(view.source, CopySource::Writer {})
        || !matches!(view.current_writer,CurrentWriter::Held {epoch} if epoch.get()==2)
        || !current_history
    {
        return Err(CanaryFailure::invalid(
            CanaryStage::Writer,
            "the declared Host is not the current e2 writer",
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CanaryStage {
    Bundle,
    Backup,
    Writer,
    Submission,
    Replay,
    Readback,
    Head,
}

pub(crate) enum CanaryFailure {
    Invalid {
        stage: CanaryStage,
        reason: &'static str,
    },
    Unknown {
        stage: CanaryStage,
    },
}
impl CanaryFailure {
    pub fn invalid(stage: CanaryStage, reason: &'static str) -> Self {
        Self::Invalid { stage, reason }
    }
}

fn next_position(cursor: FeedCursor) -> Result<FeedCursor, CanaryFailure> {
    Ok(FeedCursor {
        epoch: cursor.epoch,
        position: cursor
            .position
            .get()
            .checked_add(1)
            .and_then(|position| position.try_into().ok())
            .ok_or(CanaryFailure::invalid(
                CanaryStage::Head,
                "canary position overflow",
            ))?,
    })
}
