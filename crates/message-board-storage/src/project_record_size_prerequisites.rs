//! Minimal real related records satisfy each measured record's foreign keys.
use super::{TestResult, envelope};
use crate::ProjectWriteTransaction;
use message_board::*;

fn workstream(state: &ProjectState, id: WorkstreamId) -> TestResult<Record> {
    Ok(Record::Workstream(Workstream {
        envelope: envelope(state, id.clone().into()),
        workstream_id: id,
        project_id: state.project_id.clone(),
        name: ResourceName::try_from("native row prerequisite".to_owned())?,
        goal: String::new(),
        state: WorkstreamState::Open {},
        revision: SubjectRevision::INITIAL,
    }))
}

fn task(state: &ProjectState, id: TaskId, parent: WorkstreamId) -> TestResult<Record> {
    Ok(Record::Task(Task {
        envelope: envelope(state, id.clone().into()),
        task_id: id,
        project_id: state.project_id.clone(),
        workstream_id: parent,
        title: TaskTitle::try_from("native row prerequisite".to_owned())?,
        body: String::new(),
        status: TaskStatus::Todo,
        revision: SubjectRevision::INITIAL,
    }))
}

pub(super) async fn append(
    write: &mut ProjectWriteTransaction,
    state: &ProjectState,
    record: &mut Record,
) -> TestResult {
    match record {
        Record::Thread(row) => {
            let joined = write
                .append_record(Record::Thread(Thread {
                    envelope: envelope(state, row.thread_id.clone().into()),
                    thread_id: row.thread_id.clone(),
                    about: row.about.clone(),
                    imported: row.imported,
                    state: ThreadState::Open,
                    orchestrator: None,
                    implementer: None,
                }))
                .await?;
            let holders = [
                row.orchestrator.as_mut().map(|holder| {
                    holder.last_seen_position = joined;
                    (holder.identity.clone(), ParticipantRole::Orchestrator)
                }),
                row.implementer.as_mut().map(|holder| {
                    holder.last_seen_position = joined;
                    (holder.identity.clone(), ParticipantRole::Implementer)
                }),
            ];
            for (identity, role) in holders.into_iter().flatten() {
                write
                    .put_participant(
                        &row.thread_id,
                        &Participant::new(ParticipantProps {
                            identity,
                            role: RecordedParticipantRole::Recorded(role),
                            note: None,
                            joined_at_position: joined,
                            last_seen_position: joined,
                            closed_at_position: None,
                            closed_reason: None,
                            replaced_by: None,
                        })?,
                    )
                    .await?;
            }
        }
        Record::ThreadRefiled(row) => {
            write
                .append_record(Record::Thread(Thread {
                    envelope: envelope(state, row.thread.clone().into()),
                    thread_id: row.thread.clone(),
                    about: row.from.clone(),
                    imported: false,
                    state: ThreadState::Open,
                    orchestrator: None,
                    implementer: None,
                }))
                .await?;
        }
        Record::WorkTrailRow(row) => {
            if let TrailSubject::Workstream(id) = &row.subject {
                write.append_record(workstream(state, id.clone())?).await?;
            }
        }
        Record::TrailRedaction(row) => {
            write
                .append_record(Record::WorkTrailRow(WorkTrailRow {
                    envelope: envelope(state, row.row.clone().into()),
                    row_id: row.row.clone(),
                    project_id: state.project_id.clone(),
                    subject: TrailSubject::Project,
                    text: TrailText::try_from("native row prerequisite".to_owned())?,
                    tags: Vec::new(),
                    links: Vec::new(),
                    redacted: !row.redacted,
                }))
                .await?;
        }
        Record::Post(post) => {
            let thread_id = match &post.placement {
                Placement::Project { .. } => ThreadId::from(post.post_id.clone()),
                Placement::Thread { thread_id } => thread_id.clone(),
            };
            write
                .append_record(Record::Thread(Thread {
                    envelope: envelope(state, thread_id.clone().into()),
                    thread_id,
                    about: ThreadSubject::Project {
                        line: SubjectLine::try_from("native row size fixture".to_owned())?,
                    },
                    imported: false,
                    state: ThreadState::Open,
                    orchestrator: None,
                    implementer: None,
                }))
                .await?;
        }
        Record::Task(row) => {
            write
                .append_record(workstream(state, row.workstream_id.clone())?)
                .await?;
        }
        Record::Subtask(row) => {
            let parent = WorkstreamId::generate();
            write
                .append_record(workstream(state, parent.clone())?)
                .await?;
            write
                .append_record(task(state, row.task_id.clone(), parent)?)
                .await?;
        }
        Record::TaskLinkEvent(row) => {
            let link = match &row.change {
                TaskLinkEventKind::Added(link) | TaskLinkEventKind::Removed(link) => link,
            };
            let parent = WorkstreamId::generate();
            write
                .append_record(workstream(state, parent.clone())?)
                .await?;
            write
                .append_record(task(state, link.from.clone(), parent.clone())?)
                .await?;
            if link.to != link.from {
                write
                    .append_record(task(state, link.to.clone(), parent)?)
                    .await?;
            }
        }
        Record::RoleAssignment(row) => {
            if let RoleSubject::Workstream(id) = row.subject() {
                write.append_record(workstream(state, id)?).await?;
            }
            if let Some(grant) = &row.owner_grant {
                write
                    .append_record(Record::OwnerGrant(OwnerGrant {
                        envelope: envelope(state, grant.clone().into()),
                        id: grant.clone(),
                        grant: GrantKind::ReplaceLead {
                            project: state.project_id.clone(),
                            new_holder: row.holder.clone(),
                            reason: "native row prerequisite".to_owned(),
                        },
                        provenance: OwnerWordsProvenance::Relayed {
                            by: row.granted_by.clone(),
                        },
                    }))
                    .await?;
            }
        }
        Record::StandDownAcknowledgement(row) => {
            write
                .append_record(Record::RoleAssignment(RoleAssignment::new(
                    RoleAssignmentProps {
                        envelope: envelope(state, row.assignment.clone().into()),
                        assignment_id: row.assignment.clone(),
                        subject: RoleSubject::Project(state.project_id.clone()),
                        role: AuthorityRole::Lead,
                        holder: state.envelope.actor.clone(),
                        granted_by: state.envelope.actor.clone(),
                        model: None,
                        lineage: None,
                        ended: Some(RoleEnd::StoodDown {
                            reason: "native row prerequisite".to_owned(),
                        }),
                        owner_grant: None,
                    },
                )?))
                .await?;
        }
        _ => {}
    }
    Ok(())
}
