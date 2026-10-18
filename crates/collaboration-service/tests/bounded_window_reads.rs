//! Window scans ignore non-post bodies; feed pages batch their redaction projection.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
#[path = "support/read_cost_counter.rs"]
mod read_cost_counter;
use board_fixture::{BoardFixture, TestError};
use collaboration_service::SubscriptionWindowStore;
use message_board::*;
use read_cost_counter::{ReadCost, current_cost};
use serde_json::json;
use sqlx::{ConnectOptions, Connection};
use std::sync::Arc;
use tokio::sync::Mutex;

#[allow(clippy::panic_in_result_fn)]
async fn window_cost(size: usize) -> Result<ReadCost, TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("store")?;
    let reader: Identity =
        serde_json::from_value(json!({"kind":"human","humanId":"window reader"}))?;
    let other: Identity =
        serde_json::from_value(json!({"kind":"human","humanId":"window writer"}))?;
    let mut read = store.snapshot().await?;
    let template = read
        .record(&project.clone().into())
        .await?
        .ok_or("project")?;
    let cursor = read.high_water();
    read.finish().await?;
    let now = template.envelope().router_time;
    let envelope = |id, actor| RecordEnvelope {
        id,
        actor: RecordedActor {
            actor: ActorKind::Declared(actor),
            ..template.envelope().actor.clone()
        },
        request: None,
        position: RecordPosition::Project(cursor),
        ..template.envelope().clone()
    };
    let policy = SubscriptionPolicy::new(
        &reader,
        SubscriptionMode::Poll,
        WhenIdle::Hold,
        BatchTiming::new(0, 0)?,
        SubscriptionLifetime::defaults(),
    )?;
    let membership = SubscriptionMembership::new(SubscriptionMembershipProps {
        envelope: envelope(RecordId::generate(), reader.clone()),
        reader: reader.clone(),
        scope: SubscriptionScope::project(project.clone()),
        policy,
        renewed_at: now,
        expires_at: now + chrono::Duration::days(1),
        generation: SubscriptionGeneration::new(1)?,
        activated_at: cursor,
        ended: None,
    })?;
    let mut write = store.begin_admission().await?;
    write
        .append_record(Record::SubscriptionMembership(membership))
        .await?;
    write.set_project_watch(&reader, cursor, true).await?;
    write.commit().await?;
    let automation = Arc::new(Mutex::new(
        automation_storage::AutomationStore::open(
            &fixture._directory.path().join("automation.sqlite"),
        )
        .await?,
    ));
    let mut windows = SubscriptionWindowStore::new(fixture._registry.clone(), automation);
    assert_eq!(
        windows
            .rescan_missing_subscription_windows(&reader, now)
            .await?,
        0
    );
    let mut write = store.begin_admission().await?;
    let mut corrupt_position = None;
    for number in 0..size {
        let position = write
            .append_record(Record::Workstream(Workstream {
                envelope: envelope(workstream.clone().into(), other.clone()),
                workstream_id: workstream.clone(),
                project_id: project.clone(),
                name: format!("unrelated history {number}").try_into()?,
                goal: "not a window operand".to_owned(),
                state: WorkstreamState::Open {},
                revision: SubjectRevision::try_from(number as u64 + 1)?,
            }))
            .await?;
        corrupt_position.get_or_insert(position);
    }
    let root = PostId::generate();
    let thread = ThreadId::from(root.clone());
    write
        .append_record(Record::Thread(Thread {
            envelope: envelope(thread.clone().into(), other.clone()),
            thread_id: thread.clone(),
            about: ThreadSubject::Project {
                line: "Fixture discussion".to_owned().try_into()?,
            },
            imported: false,
            state: ThreadState::Open,
            orchestrator: None,
            implementer: None,
        }))
        .await?;
    for number in 0..5 {
        let post_id = if number == 0 {
            root.clone()
        } else {
            PostId::generate()
        };
        write
            .append_record(Record::Post(Post {
                envelope: envelope(post_id.clone().into(), other.clone()),
                post_id,
                board_id: fixture.board.clone(),
                project_id: project.clone(),
                placement: Placement::Thread {
                    thread_id: thread.clone(),
                },
                actor: other.clone(),
                acting_for: None,
                posted_as_role: None,
                text: "watched activity".to_owned().try_into()?,
                references: Vec::new().try_into()?,
                position: RecordPosition::Project(cursor),
            }))
            .await?;
    }
    write.commit().await?;
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .connect()
        .await?;
    let position = i64::try_from(corrupt_position.ok_or("corrupt position")?.position.get())?;
    let changed = sqlx::query("UPDATE project_feed SET entry = json_set(entry, '$.content.record.record.name', '') WHERE position = ?")
        .bind(position).execute(&mut connection).await?;
    assert_eq!(changed.rows_affected(), 1);
    connection.close().await?;
    let before = current_cost();
    let added = windows
        .rescan_missing_subscription_windows(&reader, now)
        .await?;
    let cost = current_cost().since(before);
    assert_eq!(
        added, 5,
        "unrelated non-post body suppressed a watched window"
    );
    assert_eq!(
        windows.due_subscription_roots(&reader, now).await?,
        vec![thread]
    );
    let mut read = store.snapshot().await?;
    assert!(
        read.feed_after(cursor, 100, false).await.is_err(),
        "direct content read must reject the same corrupt workstream"
    );
    read.finish().await?;
    fixture.stop().await?;
    Ok(cost)
}

#[allow(clippy::panic_in_result_fn)]
async fn feed_cost(size: usize) -> Result<ReadCost, TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("store")?;
    let mut read = store.snapshot().await?;
    let template = read
        .record(&project.clone().into())
        .await?
        .ok_or("project")?;
    let cursor = read.high_water();
    read.finish().await?;
    let mut write = store.begin_admission().await?;
    for _ in 0..size {
        let row_id = WorkTrailRowId::generate();
        write
            .append_record(Record::WorkTrailRow(WorkTrailRow {
                envelope: RecordEnvelope {
                    id: row_id.clone().into(),
                    request: None,
                    ..template.envelope().clone()
                },
                row_id,
                project_id: project.clone(),
                subject: TrailSubject::Project,
                text: "trail feed body".to_owned().try_into()?,
                tags: Vec::new(),
                links: Vec::new(),
                redacted: false,
            }))
            .await?;
    }
    write.commit().await?;
    let mut read = store.snapshot().await?;
    let before = current_cost();
    let page = read.feed_after(cursor, 5, false).await?;
    let cost = current_cost().since(before);
    let FeedRead::Entries { entries, .. } = page else {
        return Err("feed page".into());
    };
    assert_eq!(entries.len(), 5);
    assert!(entries.iter().all(|entry| matches!(&entry.content, FeedContent::Shown(record) if matches!(record.as_ref(), Record::WorkTrailRow(row) if !row.redacted))));
    assert_eq!(
        cost.statements, 1,
        "redaction projection must be batched with the page"
    );
    read.finish().await?;
    fixture.stop().await?;
    Ok(cost)
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn subscription_and_content_pages_bound_their_body_and_followup_reads()
-> Result<(), TestError> {
    read_cost_counter::install();
    let small = window_cost(8).await?;
    let large = window_cost(80).await?;
    eprintln!("window page costs: small={small:?}; large={large:?}");
    assert_eq!(small, large);
    assert_eq!(large.feed_entries, 5, "only the watched posts are bodies");
    let small = feed_cost(8).await?;
    let large = feed_cost(80).await?;
    eprintln!("content page costs: small={small:?}; large={large:?}");
    assert_eq!(small, large);
    Ok(())
}
