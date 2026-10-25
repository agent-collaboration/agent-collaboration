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

#[allow(clippy::panic_in_result_fn)]
async fn window_cost(size: usize) -> Result<ReadCost, TestError> {
    let mut fixture = BoardFixture::start().await?;
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
    let now = template
        .envelope()
        .time
        .as_router_time()
        .ok_or("fixture router time")?;
    let origin = fixture.record_origin()?;
    let envelope = |id, actor| RecordEnvelope {
        id,
        actor: RecordedActor::declared(actor),
        request: None,
        position: RecordPlacement::Project(ProjectFeedEntry {
            cursor,
            origin: origin.clone(),
        }),
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
    let mut write = store.begin_admission(origin.clone()).await?;
    write
        .append_record(Record::SubscriptionMembership(membership))
        .await?;
    write.set_project_watch(&reader, cursor, true).await?;
    write.commit().await?;
    let interactions = Arc::new(
        interaction_storage::InteractionStore::open(
            &fixture._directory.path().join("automation.sqlite"),
        )
        .await?,
    );
    let mut windows = SubscriptionWindowStore::new(fixture._registry.clone(), interactions);
    assert_eq!(
        windows
            .rescan_missing_subscription_windows(&reader, now)
            .await?,
        0
    );
    let mut write = store.begin_admission(origin.clone()).await?;
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
    write.commit().await?;
    let root = PostId::generate();
    let thread = ThreadId::from(root.clone());
    // Admission owns the covered-watch effect; the fixture supplies only unrelated history.
    for number in 0..5 {
        let operation = if number == 0 {
            BoardOperation::ThreadCreate {
                post_id: root.clone(),
                about: ThreadSubject::Workstream(workstream.clone()),
                text: "watched activity".to_owned().try_into()?,
                references: Vec::new(),
                participation: None,
                watch: false,
                addressees: Vec::new(),
            }
        } else {
            BoardOperation::ThreadPost {
                post_id: PostId::generate(),
                thread_id: thread.clone(),
                text: "watched activity".to_owned().try_into()?,
                references: Vec::new(),
                addressees: Vec::new(),
            }
        };
        let reply = fixture
            ._admission
            .submit(&ProjectRequest {
                request_id: RequestId::generate(),
                project_id: project.clone(),
                actor: other.clone(),
                acting_for: None,
                protocol_version: ProtocolVersion::CURRENT,
                operation,
            })
            .await?;
        assert!(matches!(
            reply.result,
            RequestResult::Decided(RequestDecision::Accepted { .. })
        ));
    }
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .connect()
        .await?;
    let position = i64::try_from(corrupt_position.ok_or("corrupt position")?.position.get())?;
    let changed = sqlx::query("UPDATE project_feed SET content = json_set(content, '$.record.record.name', '') WHERE position = ?")
        .bind(position).execute(&mut connection).await?;
    assert_eq!(changed.rows_affected(), 1);
    connection.close().await?;
    // Only the native window read is measured. Stop and join the fixture's real
    // central/session timers so their SQL cannot enter this process-wide counter.
    fixture._fleet_runtime.disconnect().await?;
    let now = chrono::Utc::now();
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
    let mut fixture = BoardFixture::start().await?;
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
    let mut write = store.begin_admission(fixture.record_origin()?).await?;
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
    fixture._fleet_runtime.disconnect().await?;
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
