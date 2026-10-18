use message_board::{
    Assignment, BoardOperation, ChangeOutcome, EpochNumber, HumanId, Identity, LinkChange,
    ProjectId, ProjectRequest, ProtocolVersion, RequestId, SubjectRevision, Task, TaskId, TaskLink,
    TaskLinkKind, TaskRefusal, TaskStatus, TaskStatusChange, TaskTitle, TrailRefusal, TrailSubject,
    WorkstreamId, request_canonical, work_rules,
};
use proptest::prelude::*;
mod support;

fn identity(sequence: u32) -> String {
    format!("01890f2e-7b4c-7cc0-98c4-{sequence:012x}")
}

#[allow(clippy::unwrap_used)]
fn task_record(sequence: u32, project: u32) -> Task {
    Task {
        envelope: support::record_envelope(
            TaskId::try_from(identity(sequence)).unwrap().into(),
            ProjectId::try_from(identity(project)).unwrap(),
        ),
        task_id: TaskId::try_from(identity(sequence)).unwrap(),
        project_id: ProjectId::try_from(identity(project)).unwrap(),
        workstream_id: WorkstreamId::try_from(identity(project + 100)).unwrap(),
        title: TaskTitle::try_from(format!("Task {sequence}")).unwrap(),
        body: String::new(),
        status: TaskStatus::Todo,
        revision: SubjectRevision::try_from(0).unwrap(),
    }
}

#[allow(clippy::unwrap_used)]
fn requester() -> Identity {
    Identity::Human {
        human_id: HumanId::try_from("writer".to_owned()).unwrap(),
    }
}

#[test]
fn task_status_table_keeps_dropped_final_and_replays_noops() {
    let statuses = [
        TaskStatus::Todo,
        TaskStatus::Doing,
        TaskStatus::Done,
        TaskStatus::Dropped,
    ];
    let task_id = TaskId::try_from(identity(1)).unwrap();
    for original in statuses {
        for requested in statuses {
            let verdict = work_rules::task_status_change(&task_id, original, requested);
            if original == requested {
                assert_eq!(verdict, Ok(ChangeOutcome::AlreadySo));
            } else if original == TaskStatus::Dropped {
                assert!(matches!(verdict, Err(TaskRefusal::Closed { .. })));
            } else {
                assert_eq!(verdict, Ok(ChangeOutcome::Changed));
            }
        }
    }
}

#[test]
fn trail_attachment_table_uses_exactly_one_assignment_or_an_explicit_subject() {
    let project = ProjectId::try_from(identity(10)).unwrap();
    let workstream = WorkstreamId::try_from(identity(11)).unwrap();
    let task = TaskId::try_from(identity(12)).unwrap();
    let cases = [
        (vec![], None, None),
        (
            vec![Assignment::Lead(project.clone())],
            None,
            Some(TrailSubject::Project),
        ),
        (
            vec![Assignment::Implementer(workstream.clone())],
            None,
            Some(TrailSubject::Workstream(workstream.clone())),
        ),
        (
            vec![Assignment::Verifier(workstream.clone())],
            None,
            Some(TrailSubject::Workstream(workstream.clone())),
        ),
        (
            vec![
                Assignment::Lead(project),
                Assignment::Implementer(workstream),
            ],
            None,
            None,
        ),
        (
            vec![],
            Some(TrailSubject::Task(task.clone())),
            Some(TrailSubject::Task(task)),
        ),
    ];
    for (assignments, explicit, expected) in cases {
        let verdict = work_rules::resolve_trail_subject(explicit, &assignments);
        match expected {
            Some(subject) => assert_eq!(verdict, Ok(subject)),
            None => assert_eq!(verdict, Err(TrailRefusal::SubjectRequired { assignments })),
        }
    }
}

#[test]
fn workstream_pair_requires_the_lead_and_distinct_identities() {
    let lead = requester();
    let other = Identity::Human {
        human_id: HumanId::try_from("other".to_owned()).unwrap(),
    };
    assert!(work_rules::require_lead(&other, Some(&lead)).is_err());
    assert!(work_rules::require_lead(&lead, Some(&lead)).is_ok());
    assert!(work_rules::require_lead(&lead, None).is_err());
    assert!(work_rules::distinct_workstream_pair(Some(&lead), Some(&lead)).is_err());
    assert!(work_rules::distinct_workstream_pair(Some(&lead), Some(&other)).is_ok());
    assert!(work_rules::distinct_workstream_pair(Some(&lead), None).is_ok());
}

#[test]
fn dependency_refusals_distinguish_missing_dropped_cross_project_and_cycle() {
    let source = task_record(1, 10);
    let target = task_record(2, 10);
    let link = TaskLink {
        from: source.task_id.clone(),
        kind: TaskLinkKind::BlockedBy,
        to: target.task_id.clone(),
    };
    assert_eq!(
        work_rules::add_task_link(&link, std::slice::from_ref(&source), &[]),
        Err(TaskRefusal::DependencyMissing(target.task_id.clone()))
    );
    let mut dropped = target.clone();
    dropped.status = TaskStatus::Dropped;
    assert_eq!(
        work_rules::add_task_link(&link, &[source.clone(), dropped], &[]),
        Err(TaskRefusal::DependencyDropped(target.task_id.clone()))
    );
    assert_eq!(
        work_rules::add_task_link(&link, &[source.clone(), task_record(2, 20)], &[]),
        Err(TaskRefusal::DependencyCrossesProject(
            target.task_id.clone()
        ))
    );
    let reverse = TaskLink {
        from: target.task_id.clone(),
        kind: TaskLinkKind::BlockedBy,
        to: source.task_id.clone(),
    };
    assert!(matches!(
        work_rules::add_task_link(&link, &[source.clone(), target.clone()], &[reverse]),
        Err(TaskRefusal::DependencyCycle { .. })
    ));
    assert_eq!(
        work_rules::add_task_link(
            &link,
            &[source.clone(), target.clone()],
            std::slice::from_ref(&link)
        ),
        Ok(LinkChange::AlreadyPresent)
    );
    let related = TaskLink {
        kind: TaskLinkKind::RelatedTo,
        ..link
    };
    assert_eq!(
        work_rules::add_task_link(&related, &[source, target], &[]),
        Ok(LinkChange::Added)
    );
}

#[allow(clippy::unwrap_used)]
fn semantic_request() -> ProjectRequest {
    ProjectRequest {
        request_id: RequestId::try_from(identity(20)).unwrap(),
        project_id: ProjectId::try_from(identity(10)).unwrap(),
        actor: requester(),
        acting_for: None,
        observed_epoch: Some(EpochNumber::try_from(1).unwrap()),
        resubmits: None,
        protocol_version: ProtocolVersion::try_from(1).unwrap(),
        operation: BoardOperation::TaskSetStatus {
            task_id: TaskId::try_from(identity(1)).unwrap(),
            status: TaskStatusChange::Doing,
            expected_revision: SubjectRevision::try_from(0).unwrap(),
        },
    }
}

#[test]
fn canonical_request_excludes_transport_identity_and_observed_epoch() {
    let first = semantic_request();
    let mut clean_switch_retry = first.clone();
    clean_switch_retry.observed_epoch = Some(EpochNumber::try_from(2).unwrap());
    assert_eq!(
        request_canonical::canonical_hash(&first).unwrap(),
        request_canonical::canonical_hash(&clean_switch_retry).unwrap()
    );
    clean_switch_retry.request_id = RequestId::try_from(identity(21)).unwrap();
    assert_eq!(
        request_canonical::canonical_hash(&first).unwrap(),
        request_canonical::canonical_hash(&clean_switch_retry).unwrap()
    );
    clean_switch_retry.operation = BoardOperation::TaskSetStatus {
        task_id: TaskId::try_from(identity(1)).unwrap(),
        status: TaskStatusChange::Done,
        expected_revision: SubjectRevision::try_from(0).unwrap(),
    };
    assert_ne!(
        request_canonical::canonical_hash(&first).unwrap(),
        request_canonical::canonical_hash(&clean_switch_retry).unwrap()
    );
    // Both the outer envelope and nested operation have a different JSON key order.
    let reordered = serde_json::from_str::<ProjectRequest>(&format!(
        r#"{{"operation":{{"expectedRevision":0,"status":{{"kind":"doing"}},"taskId":"{}","kind":"taskSetStatus"}},"protocolVersion":1,"resubmits":null,"observedEpoch":1,"actingFor":null,"actor":{{"humanId":"writer","kind":"human"}},"projectId":"{}","requestId":"{}"}}"#,
        identity(1), identity(10), identity(20),
    )).unwrap();
    assert_eq!(
        request_canonical::canonical_hash(&first).unwrap(),
        request_canonical::canonical_hash(&reordered).unwrap()
    );
}

#[test]
fn canonical_task_creation_treats_initial_links_as_sets() {
    let mut first = semantic_request();
    let create = |blocked_by, related_to| BoardOperation::TaskCreate {
        task_id: TaskId::generate(),
        workstream_id: WorkstreamId::generate(),
        title: TaskTitle::try_from("same task".to_owned()).unwrap(),
        body: String::new(),
        blocked_by,
        related_to,
    };
    let blocker = message_board::TaskLinkTarget {
        project: first.project_id.clone(),
        task: TaskId::generate(),
    };
    let other = message_board::TaskLinkTarget {
        project: first.project_id.clone(),
        task: TaskId::generate(),
    };
    first.operation = create(
        vec![blocker.clone(), other.clone()],
        vec![other, blocker.clone()],
    );
    let mut reordered = first.clone();
    if let BoardOperation::TaskCreate {
        blocked_by,
        related_to,
        ..
    } = &mut reordered.operation
    {
        blocked_by.reverse();
        related_to.reverse();
        blocked_by.push(blocker);
    }
    assert_eq!(
        request_canonical::canonical_hash(&first).unwrap(),
        request_canonical::canonical_hash(&reordered).unwrap()
    );
    reordered.acting_for = Some(message_board::ActingForIdentity::Human {
        human_id: HumanId::try_from("owner".to_owned()).unwrap(),
    });
    assert_ne!(
        request_canonical::canonical_hash(&first).unwrap(),
        request_canonical::canonical_hash(&reordered).unwrap()
    );
}

#[allow(clippy::indexing_slicing)] // Every index is in the fixed matrix's 0..6 range.
fn transitive_closure(edges: &[[bool; 6]; 6]) -> [[bool; 6]; 6] {
    let mut reachable = *edges;
    for through in 0..6 {
        for from in 0..6 {
            for to in 0..6 {
                reachable[from][to] |= reachable[from][through] && reachable[through][to];
            }
        }
    }
    reachable
}

proptest! {
    #[test]
    fn blocked_by_link_sequences_preserve_acyclicity_and_atomic_link_edits(
        edits in prop::collection::vec((0usize..6, 0usize..6, any::<bool>()), 0..100)
    ) {
        let tasks: Vec<Task> = (1..=6).map(|sequence| task_record(sequence, 10)).collect();
        let mut links = Vec::new();
        let mut edges = [[false; 6]; 6];
        for (from, to, add) in edits {
            let link = TaskLink { from: tasks[from].task_id.clone(), kind: TaskLinkKind::BlockedBy, to: tasks[to].task_id.clone() };
            if add {
                let reachable = transitive_closure(&edges);
                let result = work_rules::add_task_link(&link, &tasks, &links);
                if edges[from][to] {
                    prop_assert_eq!(result, Ok(LinkChange::AlreadyPresent));
                } else if from == to || reachable[to][from] {
                    prop_assert!(matches!(result, Err(TaskRefusal::DependencyCycle { .. })), "cycle must be refused");
                } else {
                    prop_assert_eq!(result, Ok(LinkChange::Added));
                    edges[from][to] = true;
                    links.push(link);
                }
            } else {
                let result = work_rules::remove_task_link(&link, &links);
                prop_assert_eq!(result, if edges[from][to] { LinkChange::Removed } else { LinkChange::NotPresent });
                edges[from][to] = false;
                links.retain(|existing| existing != &link);
            }
            let reachable = transitive_closure(&edges);
            prop_assert!((0..6).all(|node| !reachable[node][node]));
        }
    }
}
