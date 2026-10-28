//! The operation's rules choose keys; native queries load and validate those operands.
use super::{ProjectAdmission, ProjectAdmissionError, operation_context::OperationContext};
use chrono::{DateTime, Utc};
use message_board::{
    AuthorityRole, BoardOperation, BoardState, FeedCursor, LeaveFollowUp, ProjectRequest,
    ReadScope, Record, RecordId, RecordKind, RecordRef, RecordedActor, RoleCardinality, RoleEnd,
    RoleSubject, TaskAction, TaskLink, TaskLinkKind, ThreadId, ThreadSubject, TrailSubject,
    work_rules,
};
use message_board_storage::ProjectWriteTransaction;
use std::collections::BTreeMap;

type OperandRecords = BTreeMap<(RecordKind, RecordId), Record>;

async fn load_keys(
    write: &mut ProjectWriteTransaction,
    records: &mut OperandRecords,
    keys: Vec<RecordRef>,
) -> Result<(), ProjectAdmissionError> {
    let keys: BTreeMap<_, _> = keys
        .into_iter()
        .filter(|key| !records.contains_key(&(key.kind, key.id.clone())))
        .map(|key| ((key.kind, key.id.clone()), key))
        .collect();
    if keys.is_empty() {
        return Ok(());
    }
    for row in write
        .record_batch(&keys.into_values().collect::<Vec<_>>())
        .await?
    {
        let record = row.decode()?;
        let key = record.reference();
        records.insert((key.kind, key.id), record);
    }
    Ok(())
}
fn insert_record(records: &mut OperandRecords, record: Record) {
    let key = record.reference();
    records.insert((key.kind, key.id), record);
}

impl<'request> OperationContext<'request> {
    pub async fn load(
        admission: &ProjectAdmission,
        request: &'request ProjectRequest,
        write: &mut ProjectWriteTransaction,
        now: DateTime<Utc>,
        high_water: FeedCursor,
    ) -> Result<Self, ProjectAdmissionError> {
        let Some(Record::Project(project)) =
            write.record(&request.project_id.clone().into()).await?
        else {
            return Err(ProjectAdmissionError::InvalidRecord(
                "project state is missing".to_owned(),
            ));
        };
        let mut records = BTreeMap::new();
        insert_record(&mut records, Record::Project(project.clone()));
        let mut keys = direct_operand_keys(request);
        keys.extend(write.stood_down_actor_role_keys(&request.actor).await?);
        if needs_project_lead(&request.operation) {
            keys.extend(
                write
                    .active_role_selection_keys(
                        &RoleSubject::Project(request.project_id.clone()),
                        AuthorityRole::Lead,
                    )
                    .await?,
            );
        }
        if matches!(
            request.operation,
            BoardOperation::ThreadCreate {
                about: ThreadSubject::Project { .. },
                ..
            } | BoardOperation::ThreadRefile { .. }
        ) {
            keys.extend(write.project_role_witness_key(&request.actor).await?);
        }
        match &request.operation {
            BoardOperation::ThreadResolve { thread_id, .. }
            | BoardOperation::ThreadLeave {
                thread_id,
                after_leave: Some(LeaveFollowUp::Resolve { .. }),
            } => {
                for member in write.unresolved_thread_memberships(thread_id).await? {
                    insert_record(&mut records, Record::SubscriptionMembership(member));
                }
            }
            BoardOperation::RoleGrant {
                subject,
                role,
                holder,
                owner_grant,
                ..
            } => {
                match subject {
                    RoleSubject::Project(_) if *role == AuthorityRole::Advisor => keys.extend(
                        write
                            .active_actor_subject_role_keys(holder, subject)
                            .await?,
                    ),
                    RoleSubject::Project(_) => {
                        keys.extend(write.active_role_selection_keys(subject, *role).await?)
                    }
                    RoleSubject::Workstream(_) => {
                        for operand in [
                            AuthorityRole::Implementer,
                            AuthorityRole::Verifier,
                            AuthorityRole::Reviewer,
                        ] {
                            keys.extend(write.active_role_selection_keys(subject, operand).await?);
                        }
                        if *role == AuthorityRole::Operator {
                            keys.extend(
                                write
                                    .active_actor_subject_role_keys(holder, subject)
                                    .await?,
                            );
                        }
                    }
                }
                if *role == AuthorityRole::Lead {
                    keys.extend(write.latest_project_lead_key().await?);
                    if let Some(grant) = owner_grant {
                        keys.push(grant.clone().into());
                        keys.extend(write.owner_grant_use_key(grant).await?);
                    }
                }
            }
            BoardOperation::RoleRelease {
                subject,
                role,
                holder,
                ..
            } => {
                if *role == AuthorityRole::Operator {
                    keys.extend(
                        write
                            .active_role_selection_keys(subject, AuthorityRole::Implementer)
                            .await?,
                    );
                }
                if role.cardinality() == RoleCardinality::One {
                    keys.extend(write.active_role_selection_keys(subject, *role).await?);
                } else if let Some(identity) =
                    holder.as_ref().and_then(RecordedActor::declared_identity)
                {
                    keys.extend(
                        write
                            .active_actor_subject_role_keys(&identity, subject)
                            .await?,
                    );
                }
            }
            BoardOperation::RoleReplaceLead { owner_grant, .. } => {
                keys.extend(write.latest_project_lead_key().await?);
                keys.extend(write.owner_grant_use_key(owner_grant).await?);
            }
            BoardOperation::ProjectPublishPlan { .. } => {
                keys.extend(write.current_plan_key().await?)
            }
            BoardOperation::WorkTrailAdd { subject: None, .. } => {
                keys.extend(write.active_trail_assignment_keys(&request.actor).await?)
            }
            _ => {}
        }
        load_keys(write, &mut records, keys).await?;
        let assignments: Vec<_> = records
            .values()
            .filter_map(|record| {
                if let Record::RoleAssignment(role) = record
                    && role.holder.declared_identity().as_ref() == Some(&request.actor)
                    && matches!(role.ended, Some(RoleEnd::StoodDown { .. }))
                {
                    Some(role.assignment_id.clone())
                } else {
                    None
                }
            })
            .collect();
        let acknowledgements = write
            .stand_down_ack_keys(&request.actor, &assignments)
            .await?;
        load_keys(write, &mut records, acknowledgements).await?;
        load_trail_operands(write, &mut records, request).await?;
        if matches!(request.operation, BoardOperation::ThreadCreate { .. }) {
            let keys = write
                .latest_cooldown_post_key(&request.actor)
                .await?
                .into_iter()
                .collect();
            load_keys(write, &mut records, keys).await?;
        }
        let mut workspace_adrs = Vec::new();
        if let BoardOperation::ThreadCreate {
            about: ThreadSubject::WorkspaceAdr(id),
            ..
        }
        | BoardOperation::ThreadRefile {
            about: ThreadSubject::WorkspaceAdr(id),
            ..
        } = &request.operation
            && let Some(Record::Adr(adr)) = admission.fleet.record(&id.clone().into()).await?
        {
            workspace_adrs.push(adr);
        }
        let archived = match &project.envelope.scope.board {
            Some(board) => {
                matches!(admission.fleet.record(&board.clone().into()).await?,Some(Record::Board(board)) if matches!(board.state,BoardState::Archiving|BoardState::Archived))
            }
            None => false,
        };
        let links = task_graph_operands(write, request).await?;
        let live_subtasks = match &request.operation {
            BoardOperation::SubtaskCheck { subtask_id, .. }
            | BoardOperation::SubtaskMove { subtask_id, .. }
            | BoardOperation::SubtaskRemove { subtask_id }
                if write.live_subtask_operand(subtask_id).await? =>
            {
                vec![subtask_id.clone()]
            }
            _ => Vec::new(),
        };
        let records: Vec<_> = records.into_values().collect();
        validate_acknowledgement_cursors(write, &records).await?;
        Ok(Self {
            request,
            actor: RecordedActor::declared(request.actor.clone()),
            origin: write.origin(),
            now,
            high_water,
            admitted_epoch: write.admitted_epoch()?,
            scope: project.envelope.scope.clone(),
            project,
            records,
            links,
            live_subtasks,
            archived,
            workspace_adrs,
        })
    }
}

pub(super) async fn validate_acknowledgement_cursors(
    write: &mut ProjectWriteTransaction,
    records: &[Record],
) -> Result<(), ProjectAdmissionError> {
    let acknowledgements: Vec<_> = records
        .iter()
        .filter_map(|record| match record {
            Record::StandDownAcknowledgement(ack) => Some((record.reference(), ack.read_through)),
            _ => None,
        })
        .collect();
    let cursors: Vec<_> = acknowledgements.iter().map(|(_, cursor)| *cursor).collect();
    let validities = write.cursor_validities(&cursors).await?;
    for (record, cursor) in acknowledgements {
        if validities.get(&(cursor.epoch, cursor.position))
            != Some(&message_board_storage::CursorValidity::Valid)
        {
            return Err(message_board_storage::ProjectStorageError::InvalidRecord {
                record,
                field: "readThrough",
            }
            .into());
        }
    }
    Ok(())
}

fn needs_project_lead(operation: &BoardOperation) -> bool {
    matches!(
        operation,
        BoardOperation::WorkstreamCreate { .. }
            | BoardOperation::WorkstreamRename { .. }
            | BoardOperation::WorkstreamClose { .. }
            | BoardOperation::RoleGrant { .. }
            | BoardOperation::RoleRelease { .. }
            | BoardOperation::RoleReplaceLead { .. }
            | BoardOperation::ProjectPublishPlan { .. }
            | BoardOperation::ProjectClose { .. }
    )
}

async fn load_trail_operands(
    write: &mut ProjectWriteTransaction,
    records: &mut OperandRecords,
    request: &ProjectRequest,
) -> Result<(), ProjectAdmissionError> {
    let BoardOperation::WorkTrailAdd { subject, tags, .. } = &request.operation else {
        return Ok(());
    };
    let assignments: Vec<_> = records
        .values()
        .filter_map(|record| match record {
            Record::RoleAssignment(role)
                if role.ended.is_none()
                    && role.holder.declared_identity().as_ref() == Some(&request.actor) =>
            {
                Some(role.assignment())
            }
            _ => None,
        })
        .collect();
    let Ok(subject) = work_rules::resolve_trail_subject(subject.clone(), &assignments) else {
        return Ok(());
    };
    let keys = match &subject {
        TrailSubject::Project => Vec::new(),
        TrailSubject::Workstream(id) => vec![id.clone().into()],
        TrailSubject::Task(id) => vec![id.clone().into()],
    };
    load_keys(write, records, keys).await?;
    if tags.iter().any(|tag| tag.as_str() == "handback") {
        let workstream = match subject {
            TrailSubject::Workstream(id) => Some(id),
            TrailSubject::Task(id) => {
                records
                    .get(&(RecordKind::Task, id.into()))
                    .and_then(|record| match record {
                        Record::Task(task) => Some(task.workstream_id.clone()),
                        _ => None,
                    })
            }
            TrailSubject::Project => None,
        };
        if let Some(workstream) = workstream {
            let keys = write
                .active_actor_subject_role_keys(
                    &request.actor,
                    &RoleSubject::Workstream(workstream),
                )
                .await?;
            load_keys(write, records, keys).await?;
        }
    }
    Ok(())
}

async fn task_graph_operands(
    write: &mut ProjectWriteTransaction,
    request: &ProjectRequest,
) -> Result<Vec<TaskLink>, ProjectAdmissionError> {
    let direct = match &request.operation {
        BoardOperation::TaskCreate { blocked_by, .. }
            if blocked_by
                .iter()
                .any(|target| target.task_in_project(&request.project_id).is_ok()) =>
        {
            // The existing pure cycle rule validates this project's typed link graph.
            // Endpoint bodies remain ID-scoped; unrelated task bodies are never loaded.
            return Ok(write.task_links().await?);
        }
        BoardOperation::TaskLink { link } => match link.link_in_project(&request.project_id) {
            Ok(link) if link.kind == TaskLinkKind::BlockedBy => {
                return Ok(write.task_links().await?);
            }
            Ok(link) => Some(link),
            Err(_) => None,
        },
        BoardOperation::TaskUnlink { link } => Some(link.clone()),
        _ => None,
    };
    match direct {
        Some(link) => Ok(write.task_link_operand(&link).await?.into_iter().collect()),
        None => Ok(Vec::new()),
    }
}

fn direct_operand_keys(request: &ProjectRequest) -> Vec<RecordRef> {
    match &request.operation {
        BoardOperation::NotificationCancel { .. } => Vec::new(),
        BoardOperation::WorkstreamCreate { workstream_id, .. }
        | BoardOperation::WorkstreamRename { workstream_id, .. }
        | BoardOperation::WorkstreamClose { workstream_id, .. } => {
            vec![workstream_id.clone().into()]
        }
        BoardOperation::TaskCreate {
            task_id,
            workstream_id,
            blocked_by,
            related_to,
            ..
        } => {
            let mut keys = vec![task_id.clone().into(), workstream_id.clone().into()];
            keys.extend(
                blocked_by
                    .iter()
                    .chain(related_to)
                    .filter_map(|target| target.task_in_project(&request.project_id).ok())
                    .map(RecordRef::from),
            );
            keys
        }
        BoardOperation::TaskUpdate { task_id, .. }
        | BoardOperation::TaskSetStatus { task_id, .. } => vec![task_id.clone().into()],
        BoardOperation::TaskMoveWorkstream {
            task_id,
            workstream_id,
            ..
        } => vec![task_id.clone().into(), workstream_id.clone().into()],
        BoardOperation::TaskLink { link } => {
            let mut keys = vec![link.from.clone().into()];
            if let Ok(task) = link.to.task_in_project(&request.project_id) {
                keys.push(task.into());
            }
            keys
        }
        BoardOperation::TaskUnlink { link } => vec![link.from.clone().into()],
        BoardOperation::TaskAction {
            task_id, action, ..
        } => vec![
            task_id.clone().into(),
            match action {
                TaskAction::Duplicate { of, .. } => of.clone().into(),
                TaskAction::Replace { by, .. } => by.clone().into(),
            },
        ],
        BoardOperation::SubtaskAdd {
            subtask_id,
            task_id,
            ..
        } => vec![subtask_id.clone().into(), task_id.clone().into()],
        BoardOperation::SubtaskCheck { subtask_id, .. }
        | BoardOperation::SubtaskMove { subtask_id, .. }
        | BoardOperation::SubtaskRemove { subtask_id } => vec![subtask_id.clone().into()],
        BoardOperation::WorkTrailAdd { row_id, .. }
        | BoardOperation::WorkTrailRedact { row_id, .. } => vec![row_id.clone().into()],
        BoardOperation::RoleGrant {
            subject: RoleSubject::Workstream(id),
            ..
        }
        | BoardOperation::RoleRelease {
            subject: RoleSubject::Workstream(id),
            ..
        } => vec![id.clone().into()],
        BoardOperation::OwnerGrantRecord { grant_id, .. } => vec![grant_id.clone().into()],
        BoardOperation::RoleReplaceLead { owner_grant, .. } => vec![owner_grant.clone().into()],
        BoardOperation::FactRecord { fact } => vec![fact.fact_id.clone().into()],
        BoardOperation::ThreadCreate { post_id, about, .. } => {
            let mut keys = vec![
                post_id.clone().into(),
                ThreadId::from(post_id.clone()).into(),
            ];
            keys.extend(thread_subject_keys(about));
            keys
        }
        BoardOperation::ThreadRefile { thread_id, about } => {
            let mut keys = vec![thread_id.clone().into()];
            keys.extend(thread_subject_keys(about));
            keys
        }
        BoardOperation::ThreadPost {
            post_id, thread_id, ..
        } => vec![post_id.clone().into(), thread_id.clone().into()],
        BoardOperation::ThreadResolve { thread_id, .. }
        | BoardOperation::ThreadUnresolve { thread_id }
        | BoardOperation::ThreadJoin { thread_id, .. }
        | BoardOperation::ThreadLeave { thread_id, .. }
        | BoardOperation::ThreadWatch { thread_id }
        | BoardOperation::ThreadUnwatch { thread_id } => vec![thread_id.clone().into()],
        BoardOperation::ThreadSubscribe { scope, .. }
        | BoardOperation::ThreadUnsubscribe { scope }
        | BoardOperation::ThreadRenew { scope } => scope
            .thread_id()
            .cloned()
            .map(RecordRef::from)
            .into_iter()
            .collect(),
        BoardOperation::ReadMarkInitialize { scope, .. }
        | BoardOperation::ReadMarkAdvance { scope, .. } => match scope {
            ReadScope::Thread { thread_id } => vec![thread_id.clone().into()],
            ReadScope::Project { .. } => Vec::new(),
        },
        BoardOperation::RoleGrant { .. }
        | BoardOperation::RoleRelease { .. }
        | BoardOperation::RoleAcknowledgeCurrent { .. }
        | BoardOperation::ProjectPublishPlan { .. }
        | BoardOperation::ProjectClose { .. }
        | BoardOperation::ProjectDescribe { .. }
        | BoardOperation::ProjectWatch {}
        | BoardOperation::ProjectUnwatch {} => Vec::new(),
    }
}

fn thread_subject_keys(subject: &ThreadSubject) -> Vec<RecordRef> {
    match subject {
        ThreadSubject::Project { .. } | ThreadSubject::WorkspaceAdr(_) => Vec::new(),
        ThreadSubject::Workstream(id) => vec![id.clone().into()],
        ThreadSubject::Task(id) => vec![id.clone().into()],
        ThreadSubject::WorkTrailRow(id) => vec![id.clone().into()],
    }
}
