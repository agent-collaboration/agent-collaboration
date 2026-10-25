//! Rule operands and metadata counts must not decode unrelated project contents.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
#[path = "support/read_cost_counter.rs"]
mod read_cost_counter;
use board_fixture::{BoardFixture, TestError, accepted};
use message_board::*;
use read_cost_counter::{ReadCost, current_cost};
use serde_json::{Value, json};
use std::collections::BTreeMap;

async fn measured_write(
    fixture: &BoardFixture,
    tool: &str,
    request: Value,
) -> Result<ReadCost, TestError> {
    let before = current_cost();
    let reply = fixture.api.call(tool, request).await?;
    let cost = current_cost().since(before);
    accepted(&reply);
    Ok(cost)
}

async fn write_costs(size: usize) -> Result<BTreeMap<&'static str, ReadCost>, TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let task = fixture.task(&project, &workstream).await?;
    let post = PostId::generate();
    let thread = ThreadId::from(post.clone());
    let mut root = fixture.header();
    root.as_object_mut().ok_or("header")?.extend(
        json!({"projectId":project,"postId":post,
        "about":{"kind":"workstream","subject":workstream},"role":"orchestrator","watch":false,"text":"write operand fixture",
        "references":[],"addressees":[]})
        .as_object()
        .ok_or("fields")?
        .clone(),
    );
    accepted(&fixture.api.call("thread_create", root).await?);
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    let template = read
        .record(&project.clone().into())
        .await?
        .ok_or("project record")?;
    let cursor = read.high_water();
    read.finish().await?;
    let origin = fixture.record_origin()?;
    let envelope = |id: RecordId| RecordEnvelope {
        id,
        request: None,
        position: RecordPlacement::Project(ProjectFeedEntry {
            cursor,
            origin: origin.clone(),
        }),
        ..template.envelope().clone()
    };
    let other = Identity::Human {
        human_id: "unrelated holder".to_owned().try_into()?,
    };
    let creating_actor = Identity::Human {
        human_id: "creating writer".to_owned().try_into()?,
    };
    let mut write = store.begin_admission(origin.clone()).await?;
    let mut link_target = None;
    for number in 0..size {
        let root_post = PostId::generate();
        let mut old_envelope = envelope(root_post.clone().into());
        old_envelope.actor = RecordedActor::declared(creating_actor.clone());
        old_envelope.time = RecordTime::Router(
            old_envelope
                .time
                .as_router_time()
                .ok_or("fixture router time")?
                - chrono::Duration::seconds(120),
        );
        write
            .append_record(Record::Thread(Thread {
                envelope: old_envelope.clone(),
                thread_id: ThreadId::from(root_post.clone()),
                about: ThreadSubject::Project {
                    line: "Fixture discussion".to_owned().try_into()?,
                },
                imported: false,
                state: ThreadState::Open,
                orchestrator: None,
                implementer: None,
            }))
            .await?;
        write
            .append_record(Record::Post(Post {
                envelope: old_envelope,
                post_id: root_post,
                board_id: fixture.board.clone(),
                project_id: project.clone(),
                placement: Placement::Project {
                    project_id: project.clone(),
                },
                actor: creating_actor.clone(),
                acting_for: None,
                posted_as_role: None,
                text: "old cooldown history".to_owned().try_into()?,
                references: Vec::new().try_into()?,
                position: RecordPlacement::Project(ProjectFeedEntry {
                    cursor,
                    origin: origin.clone(),
                }),
            }))
            .await?;
        let workstream_id = WorkstreamId::generate();
        write
            .append_record(Record::Workstream(Workstream {
                envelope: envelope(workstream_id.clone().into()),
                workstream_id: workstream_id.clone(),
                project_id: project.clone(),
                name: format!("unrelated {number}").try_into()?,
                goal: "unrelated work".to_owned(),
                state: WorkstreamState::Open {},
                revision: SubjectRevision::INITIAL,
            }))
            .await?;
        let assignment = RoleAssignmentId::generate();
        write
            .append_record(Record::RoleAssignment(RoleAssignment::new(
                RoleAssignmentProps {
                    envelope: envelope(assignment.clone().into()),
                    assignment_id: assignment,
                    subject: RoleSubject::Workstream(workstream_id),
                    role: AuthorityRole::Implementer,
                    holder: RecordedActor::declared(other.clone()),
                    granted_by: template.envelope().actor.clone(),
                    model: None,
                    lineage: None,
                    ended: None,
                    owner_grant: None,
                },
            )?))
            .await?;
        let task_id = TaskId::generate();
        link_target.get_or_insert_with(|| task_id.clone());
        write
            .append_record(Record::Task(Task {
                envelope: envelope(task_id.clone().into()),
                task_id,
                project_id: project.clone(),
                workstream_id: workstream.clone(),
                title: format!("unrelated task {number}").try_into()?,
                body: "unrelated task body".to_owned(),
                status: TaskStatus::Todo,
                revision: SubjectRevision::INITIAL,
            }))
            .await?;
        let post_id = PostId::generate();
        write
            .append_record(Record::Post(Post {
                envelope: RecordEnvelope {
                    actor: RecordedActor::declared(other.clone()),
                    ..envelope(post_id.clone().into())
                },
                post_id,
                board_id: fixture.board.clone(),
                project_id: project.clone(),
                placement: Placement::Thread {
                    thread_id: thread.clone(),
                },
                actor: other.clone(),
                acting_for: None,
                posted_as_role: None,
                text: "unrelated post".to_owned().try_into()?,
                references: Vec::new().try_into()?,
                position: RecordPlacement::Project(ProjectFeedEntry {
                    cursor,
                    origin: origin.clone(),
                }),
            }))
            .await?;
        let row_id = WorkTrailRowId::generate();
        write
            .append_record(Record::WorkTrailRow(WorkTrailRow {
                envelope: envelope(row_id.clone().into()),
                row_id,
                project_id: project.clone(),
                subject: TrailSubject::Project,
                text: "unrelated trail".to_owned().try_into()?,
                tags: Vec::new(),
                links: Vec::new(),
                redacted: false,
            }))
            .await?;
        let fact_id = FactId::generate();
        write
            .append_record(Record::Fact(Fact {
                envelope: envelope(fact_id.clone().into()),
                fact_id,
                kind: FactKind::EvaluatorResult {
                    guard: format!("unused guard {number}"),
                    subject: project.clone().into(),
                    outcome: GuardOutcome::Met,
                    evidence: Vec::new(),
                    evaluator: EvaluatorRef {
                        actor: template.envelope().actor.clone(),
                        engine: None,
                        calibration: None,
                    },
                    executor: None,
                },
                links: vec![project.clone().into()],
                producer_time: template
                    .envelope()
                    .time
                    .as_router_time()
                    .ok_or("fixture router time")?,
                anchor: Some(cursor),
                late: false,
            }))
            .await?;
    }
    write.commit().await?;
    let evaluator = fixture.actor.clone();
    accepted(&fixture.api.call("fact_record",fixture.request(&project,json!({"kind":"factRecord","fact":{"factId":FactId::generate(),"kind":{"kind":"evaluatorResult","fact":{"guard":"proof","subject":{"kind":"project","id":project},"outcome":"met","evidence":[],"evaluator":{"actor":evaluator,"engine":null,"calibration":null},"executor":null}},"links":[{"kind":"project","id":project}],"producerTime":"2026-10-12T00:00:00Z","anchor":null}}))).await?);
    let target = link_target.ok_or("link target")?;
    let mut costs = BTreeMap::new();
    for (family, tool, operation) in [
        (
            "workstream",
            "workstream_rename",
            json!({"kind":"workstreamRename","workstreamId":workstream,"name":"target rename","expectedRevision":0}),
        ),
        (
            "task",
            "task_update",
            json!({"kind":"taskUpdate","taskId":task,"title":"target update","body":"target","expectedRevision":0}),
        ),
        (
            "subtask",
            "subtask_add",
            json!({"kind":"subtaskAdd","subtaskId":SubtaskId::generate(),"taskId":task,"text":"target child","place":"80"}),
        ),
        (
            "graph",
            "task_link",
            json!({"kind":"taskLink","link":{"from":task,"kind":"blockedBy","to":{"task":target,"project":project}}}),
        ),
        (
            "trail",
            "work_trail_add",
            json!({"kind":"workTrailAdd","rowId":WorkTrailRowId::generate(),"subject":{"kind":"project"},"text":"target trail","tags":[],"links":[]}),
        ),
        (
            "role",
            "role_grant",
            json!({"kind":"roleGrant","subject":{"kind":"workstream","subject":workstream},"role":"implementer","holder":other,"model":null,"lineage":null,"ownerGrant":null,"expectedRevision":0}),
        ),
        (
            "plan",
            "project_publish_plan",
            json!({"kind":"projectPublishPlan","hash":"77".repeat(32),"document":{"kind":"url","url":"https://example.com/operand-plan"},"expectedRevision":0}),
        ),
    ] {
        costs.insert(
            family,
            measured_write(&fixture, tool, fixture.request(&project, operation)).await?,
        );
    }
    let mut request = fixture.header();
    request.as_object_mut().ok_or("header")?.extend(json!({"postId":PostId::generate(),"placement":{"kind":"thread","thread":{"thread":thread}},"text":"target reply","references":[],"addressees":[]}).as_object().ok_or("fields")?.clone());
    costs.insert(
        "thread",
        measured_write(&fixture, "thread_post", request).await?,
    );
    let mut request = fixture.header();
    request.as_object_mut().ok_or("header")?.extend(
        json!({"thread":{"thread":thread}})
            .as_object()
            .ok_or("fields")?
            .clone(),
    );
    costs.insert(
        "membership",
        measured_write(&fixture, "thread_watch", request).await?,
    );
    let mut request = fixture.header();
    request.as_object_mut().ok_or("header")?.extend(json!({"actor":creating_actor,"projectId":project,"postId":PostId::generate(),
        "about":{"kind":"workstream","subject":workstream},"role":null,"watch":false,"text":"new root after cooldown","references":[],"addressees":[]}).as_object().ok_or("fields")?.clone());
    costs.insert(
        "threadCreate",
        measured_write(&fixture, "thread_create", request).await?,
    );
    fixture.stop().await?;
    Ok(costs)
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn representative_writes_have_operand_bounded_statement_and_decode_counts()
-> Result<(), TestError> {
    read_cost_counter::install();
    let small = write_costs(8).await?;
    let large = write_costs(80).await?;
    eprintln!("write costs: small={small:?}\nlarge={large:?}");
    assert_eq!(
        small, large,
        "write work grew with unrelated project records"
    );
    for (family, cost) in large {
        assert!(cost.statements > 0, "{family}: missing real statements");
        assert!(
            cost.records <= 8,
            "{family}: decoded beyond its named rule operands: {cost:?}"
        );
        assert!(
            cost.participants <= 1 && cost.feed_entries <= 2,
            "{family}: decoded unrelated conversation history: {cost:?}"
        );
    }
    Ok(())
}
