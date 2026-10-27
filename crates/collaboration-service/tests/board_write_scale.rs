#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! Release write latency at the largest-board record counts, through real MCP admission.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
#[cfg(not(debug_assertions))]
#[path = "support/write_family_timing.rs"]
mod write_family_timing;
use board_fixture::{BoardFixture, TestError, accepted};
use message_board::*;
use serde_json::json;
use std::time::Instant;

const SAMPLES: usize = 50;
fn p95(samples: &mut [u128]) -> Result<u128, TestError> {
    samples.sort_unstable();
    samples
        .get((SAMPLES * 95).div_ceil(100) - 1)
        .copied()
        .ok_or_else(|| "missing p95 sample".into())
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn, clippy::indexing_slicing)] // Test-owned JSON construction; samples use checked access.
async fn real_size_project_measures_admitted_posts_and_task_updates() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let task = fixture.task(&project, &workstream).await?;
    let root_post = PostId::generate();
    let thread = ThreadId::from(root_post.clone());
    let mut root = fixture.header();
    root["projectId"] = json!(project);
    root["postId"] = json!(root_post);
    root["about"] = json!({"kind":"workstream","subject":workstream});
    root["role"] = json!(null);
    root["watch"] = json!(false);
    root["text"] = json!("scale profile conversation");
    root["references"] = json!([]);
    root["addressees"] = json!([]);
    accepted(&fixture.api.call("thread_create", root).await?);
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    let base = read
        .record(&project.clone().into())
        .await?
        .ok_or("project record")?;
    let cursor = read.high_water();
    read.finish().await?;
    let origin = fixture.record_origin()?;
    let writer = Identity::Human {
        human_id: HumanId::try_from("history writer".to_owned())?,
    };
    let envelope = |id: RecordId| RecordEnvelope {
        id,
        request: None,
        position: RecordPlacement::Project(ProjectFeedEntry {
            cursor,
            origin: origin.clone(),
        }),
        actor: RecordedActor::declared(writer.clone()),
        ..base.envelope().clone()
    };
    // Seed validated historical records on the actual capturing native store. Measurements
    // below use fresh keyed public admission, rather than timing this fixture import.
    let mut write = store.begin_admission(origin.clone()).await?;
    for number in 1..2500 {
        let id = PostId::generate();
        write
            .append_record(Record::Post(Post {
                envelope: envelope(id.clone().into()),
                post_id: id,
                board_id: fixture.board.clone(),
                project_id: project.clone(),
                placement: Placement::Thread {
                    thread_id: thread.clone(),
                },
                actor: writer.clone(),
                acting_for: None,
                posted_as_role: None,
                text: PostText::try_from(format!(
                    "historical post {number}: {}",
                    "recorded work and evidence. ".repeat(10)
                ))?,
                references: PostReferences::try_from(Vec::new())?,
                position: RecordPlacement::Project(ProjectFeedEntry {
                    cursor,
                    origin: origin.clone(),
                }),
            }))
            .await?;
    }
    for number in 1..200 {
        let id = TaskId::generate();
        write
            .append_record(Record::Task(Task {
                envelope: envelope(id.clone().into()),
                task_id: id,
                project_id: project.clone(),
                workstream_id: workstream.clone(),
                title: TaskTitle::try_from(format!("historical task {number}"))?,
                body: "recorded task body".repeat(10),
                status: TaskStatus::Todo,
                revision: SubjectRevision::INITIAL,
            }))
            .await?;
    }
    for number in 0..500 {
        let id = WorkTrailRowId::generate();
        write
            .append_record(Record::WorkTrailRow(WorkTrailRow {
                envelope: envelope(id.clone().into()),
                row_id: id,
                project_id: project.clone(),
                subject: TrailSubject::Project,
                text: TrailText::try_from(format!(
                    "historical trail {number}: {}",
                    "progress and evidence. ".repeat(10)
                ))?,
                tags: vec![TrailTag::try_from("scale-profile".to_owned())?],
                links: Vec::new(),
                redacted: false,
            }))
            .await?;
    }
    write.commit().await?;
    let mut read = store.snapshot().await?;
    assert_eq!(read.records(RecordKind::Post).await?.len(), 2500);
    assert_eq!(read.records(RecordKind::Task).await?.len(), 200);
    assert_eq!(read.records(RecordKind::WorkTrailRow).await?.len(), 500);
    let size_head = read.high_water();
    read.finish().await?;
    let mut post_times = Vec::new();
    let mut task_times = Vec::new();
    for sample in 0..SAMPLES {
        let mut post = fixture.header();
        post["postId"] = json!(PostId::generate());
        post["placement"] = json!({"kind":"thread","thread":{"thread":thread}});
        post["text"] = json!(format!("measured admitted post {sample}"));
        post["references"] = json!([]);
        post["addressees"] = json!([]);
        let started = Instant::now();
        let reply = fixture.api.call("thread_post", post).await?;
        post_times.push(started.elapsed().as_micros());
        accepted(&reply);
        let update = fixture.request(&project, json!({"kind":"taskUpdate","taskId":task,
            "title":format!("measured task update {sample}"),"body":"updated body","expectedRevision":sample}));
        let started = Instant::now();
        let reply = fixture.api.call("task_update", update).await?;
        task_times.push(started.elapsed().as_micros());
        accepted(&reply);
    }
    let post_p95 = p95(&mut post_times)?;
    let task_p95 = p95(&mut task_times)?;
    eprintln!(
        "scale profile: release={} initial posts=2500 tasks=200 trail=500 feed={} samples={} post_p95_us={} task_p95_us={}",
        !cfg!(debug_assertions),
        size_head.position.get(),
        SAMPLES,
        post_p95,
        task_p95
    );
    assert_eq!(post_times.len(), SAMPLES);
    assert_eq!(task_times.len(), SAMPLES);
    let mut read = store.snapshot().await?;
    let Some(Record::Task(updated)) = read.record(&task.clone().into()).await? else {
        return Err("updated task missing".into());
    };
    assert_eq!(updated.revision.get(), u64::try_from(SAMPLES)?);
    assert_eq!(read.records(RecordKind::Post).await?.len(), 2500 + SAMPLES);
    read.finish().await?;
    if !cfg!(debug_assertions) {
        assert!(
            post_p95 < 50_000,
            "release admitted-post p95 {post_p95}us exceeds 50ms"
        );
        assert!(
            task_p95 < 50_000,
            "release task-update p95 {task_p95}us exceeds 50ms"
        );
    }
    #[cfg(not(debug_assertions))]
    write_family_timing::measure_remaining_families(
        &fixture,
        &project,
        &workstream,
        &task,
        &thread,
    )
    .await?;
    fixture.stop().await
}
