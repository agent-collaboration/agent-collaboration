//! Populate valid epoch-two history through ordinary public project mutations.
use super::{offline_cli_fixture::TestResult, recovery_rows::require};
use collaboration_client::CollaborationClient;
use message_board::{
    AssignmentRevision, AuthorityRole, BoardOperation, EvidencePointer, FactId, FactInput,
    FactKind, HumanId, Identity, PrFactKind, ProjectId, ProjectRequest, ProtocolVersion,
    RecordedActor, RequestId, RoleSubject, SubjectRevision, SubtaskId, TaskId, TaskLinkInput,
    TaskLinkKind, TaskLinkTarget, WorkstreamId,
    public_replies::{RequestDecision, RequestResult, WriteReply},
};
use sha2::{Digest, Sha256};

pub(super) const LARGE_PR_NUMBER: u64 = 9_007_199_254_740_993;
pub(super) const UPDATED_TASK_BODY: &str =
    "preserve \"quotes\", \\slashes and Unicode 🐒\nsecond line";

pub(super) struct RichRecords {
    pub project: ProjectId,
    pub actor: Identity,
    pub workstream: WorkstreamId,
    pub first_task: TaskId,
    pub second_task: TaskId,
    pub subtask: SubtaskId,
    pub fact: FactId,
    pub replay: ProjectRequest,
    pub replay_result: RequestResult,
}

pub(super) async fn seed(
    client: &CollaborationClient,
    project: ProjectId,
    owner: &HumanId,
) -> TestResult<RichRecords> {
    let actor = Identity::Human {
        human_id: owner.clone(),
    };
    let workstream = WorkstreamId::generate();
    let first_task = TaskId::generate();
    let second_task = TaskId::generate();
    let subtask = SubtaskId::generate();
    let fact = FactId::generate();
    accepted(
        client
            .role_grant(request(
                &project,
                &actor,
                BoardOperation::RoleGrant {
                    subject: RoleSubject::Project(project.clone()),
                    role: AuthorityRole::Lead,
                    holder: actor.clone(),
                    model: None,
                    lineage: None,
                    expected_revision: AssignmentRevision::INITIAL,
                    owner_grant: None,
                },
            ))
            .await?,
        "source self-lead grant",
    )?;
    accepted(
        client
            .workstream_create(request(
                &project,
                &actor,
                BoardOperation::WorkstreamCreate {
                    workstream_id: workstream.clone(),
                    name: "preserved workstream".to_owned().try_into()?,
                    goal: "retain work and dependencies".to_owned(),
                },
            ))
            .await?,
        "source workstream",
    )?;
    for (role, revision) in [
        (AuthorityRole::Implementer, 0_u64),
        (AuthorityRole::Verifier, 1_u64),
    ] {
        accepted(
            client
                .role_grant(request(
                    &project,
                    &actor,
                    BoardOperation::RoleGrant {
                        subject: RoleSubject::Workstream(workstream.clone()),
                        role,
                        holder: Identity::Human {
                            human_id: uuid::Uuid::now_v7().to_string().try_into()?,
                        },
                        model: None,
                        lineage: None,
                        expected_revision: revision.try_into()?,
                        owner_grant: None,
                    },
                ))
                .await?,
            "source workstream role",
        )?;
    }
    for (task, title) in [(&first_task, "first task"), (&second_task, "second task")] {
        accepted(
            client
                .task_create(request(
                    &project,
                    &actor,
                    BoardOperation::TaskCreate {
                        task_id: task.clone(),
                        workstream_id: workstream.clone(),
                        title: title.to_owned().try_into()?,
                        body: "original task body".to_owned(),
                        blocked_by: vec![],
                        related_to: vec![],
                    },
                ))
                .await?,
            "source task",
        )?;
    }
    let replay = request(
        &project,
        &actor,
        BoardOperation::TaskUpdate {
            task_id: first_task.clone(),
            title: "first task revised".to_owned().try_into()?,
            body: UPDATED_TASK_BODY.to_owned(),
            expected_revision: SubjectRevision::INITIAL,
        },
    );
    let replay_result = accepted(
        client.task_update(replay.clone()).await?,
        "source task history",
    )?;
    accepted(
        client
            .task_link(request(
                &project,
                &actor,
                BoardOperation::TaskLink {
                    link: TaskLinkInput {
                        from: second_task.clone(),
                        kind: TaskLinkKind::BlockedBy,
                        to: TaskLinkTarget {
                            project: project.clone(),
                            task: first_task.clone(),
                        },
                    },
                },
            ))
            .await?,
        "source dependency",
    )?;
    accepted(
        client
            .task_link(request(
                &project,
                &actor,
                BoardOperation::TaskLink {
                    link: TaskLinkInput {
                        from: first_task.clone(),
                        kind: TaskLinkKind::RelatedTo,
                        to: TaskLinkTarget {
                            project: project.clone(),
                            task: second_task.clone(),
                        },
                    },
                },
            ))
            .await?,
        "source related link",
    )?;
    accepted(
        client
            .subtask_add(request(
                &project,
                &actor,
                BoardOperation::SubtaskAdd {
                    subtask_id: subtask.clone(),
                    task_id: first_task.clone(),
                    text: "preserved checklist item".to_owned().try_into()?,
                    place: "80".to_owned().try_into()?,
                },
            ))
            .await?,
        "source subtask",
    )?;
    accepted(
        client
            .subtask_check(request(
                &project,
                &actor,
                BoardOperation::SubtaskCheck {
                    subtask_id: subtask.clone(),
                    checked: true,
                },
            ))
            .await?,
        "source checked subtask",
    )?;
    accepted(
        client
            .fact_record(request(
                &project,
                &actor,
                BoardOperation::FactRecord {
                    fact: Box::new(FactInput {
                        fact_id: fact.clone(),
                        kind: FactKind::PrFact {
                            repository: "github.com/example/c5b-proof".to_owned().try_into()?,
                            number: LARGE_PR_NUMBER,
                            kind: PrFactKind::HeadObserved {
                                head: "1".repeat(40).try_into()?,
                            },
                            recorded_by: RecordedActor::declared(actor.clone()),
                        },
                        links: vec![project.clone().into(), first_task.clone().into()],
                        producer_time: chrono::DateTime::from_timestamp(1_577_934_245, 0)
                            .ok_or("fixed producer time invalid")?,
                        anchor: None,
                    }),
                },
            ))
            .await?,
        "source fact",
    )?;
    accepted(
        client
            .project_describe(request(
                &project,
                &actor,
                BoardOperation::ProjectDescribe {
                    description: "rich epoch-two source".to_owned().try_into()?,
                    expected_revision: SubjectRevision::INITIAL,
                },
            ))
            .await?,
        "source project description",
    )?;
    for (revision, text) in [(1_u64, "plan version one"), (2_u64, "plan version two")] {
        accepted(
            client
                .project_publish_plan(request(
                    &project,
                    &actor,
                    BoardOperation::ProjectPublishPlan {
                        hash: format!("{:x}", Sha256::digest(text.as_bytes())).try_into()?,
                        document: EvidencePointer::Record {
                            record: fact.clone().into(),
                        },
                        expected_revision: revision.try_into()?,
                    },
                ))
                .await?,
            "source plan revision",
        )?;
    }
    Ok(RichRecords {
        project,
        actor,
        workstream,
        first_task,
        second_task,
        subtask,
        fact,
        replay,
        replay_result,
    })
}

pub(super) fn request(
    project: &ProjectId,
    actor: &Identity,
    operation: BoardOperation,
) -> ProjectRequest {
    ProjectRequest {
        request_id: RequestId::generate(),
        project_id: project.clone(),
        actor: actor.clone(),
        acting_for: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation,
    }
}

pub(super) fn accepted(reply: WriteReply, context: &str) -> TestResult<RequestResult> {
    require(
        matches!(
            reply.result,
            RequestResult::Decided(RequestDecision::Accepted { .. })
        ),
        &format!("{context} was refused: {:?}", reply.result),
    )?;
    Ok(reply.result)
}
