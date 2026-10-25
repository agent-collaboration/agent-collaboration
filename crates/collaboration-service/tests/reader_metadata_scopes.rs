//! Thread write metadata reads the thread's activity and still checks the scanned line.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError, accepted};
use message_board::*;
use serde_json::{Value, json};
use sqlx::{ConnectOptions, Connection};

async fn create(
    fixture: &BoardFixture,
    project: &ProjectId,
    post: &PostId,
    workstream: &WorkstreamId,
    actor: &str,
) -> Result<(Value, Value), TestError> {
    let mut request = fixture.header();
    request.as_object_mut().ok_or("header")?.extend(json!({"projectId":project,"postId":post,"actor":{"kind":"human","humanId":actor},"text":"scoped thread","about":{"kind":"workstream","subject":workstream},"references":[],"addressees":[],"role":null,"watch":false}).as_object().ok_or("fields")?.clone());
    Ok((
        request.clone(),
        fixture.api.call("thread_create", request).await?,
    ))
}
#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn thread_metadata_skips_bodies_while_content_reads_refuse_corruption()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, lane) = fixture.project().await?;
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    let zero = FeedCursor {
        epoch: read.high_water().epoch,
        position: FeedPosition::ZERO,
    };
    let FeedRead::Entries { entries, .. } = read.feed_after(zero, u64::MAX, true).await? else {
        return Err("feed".into());
    };
    let entry = entries
        .into_iter()
        .find(|entry| entry.record == RecordRef::from(lane.clone()))
        .ok_or("lane")?;
    let mut payload = serde_json::to_value(entry)?;
    *payload
        .pointer_mut("/content/record/record/name")
        .ok_or("name")? = json!(" padded off-thread name ");
    read.finish().await?;
    let mut observer = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .connect()
        .await?;
    sqlx::query("UPDATE project_feed SET content=? WHERE record_id=?")
        .bind(payload.get("content").ok_or("content")?.to_string())
        .bind(lane.as_str())
        .execute(&mut observer)
        .await?;
    let post = PostId::generate();
    let (request, reply) = create(&fixture, &project, &post, &lane, "scoped writer").await?;
    accepted(&reply);
    assert_eq!(
        reply.pointer("/result/reader/unreadCount"),
        Some(&json!(3)),
        "{reply}"
    );
    let explicit=fixture.api.call("feed_read",json!({"projectId":project,"reader":{"kind":"anonymous"},"after":zero,"limit":100,"includeRedacted":true})).await?;
    assert_eq!(
        explicit.pointer("/error/data/kind"),
        Some(&json!("invalidRecord"))
    );
    let mut read = store.snapshot().await?;
    let selected = read
        .record(&post.clone().into())
        .await?
        .ok_or("selected post")?;
    read.finish().await?;
    let mut selected_payload = serde_json::to_value(&selected)?;
    *selected_payload
        .pointer_mut("/record/envelope/position/position/origin/machine")
        .ok_or("machine")? = json!(" V1/V2 proof ");
    // The actual selected immutable entry, rather than an unrelated current record, must fail.
    let raw: String = sqlx::query_scalar(
        "SELECT content FROM project_feed WHERE record_kind='post' AND record_id=?",
    )
    .bind(post.as_str())
    .fetch_one(&mut observer)
    .await?;
    let mut raw: Value = serde_json::from_str(&raw)?;
    *raw.pointer_mut("/record").ok_or("record")? = selected_payload;
    sqlx::query("UPDATE project_feed SET content=? WHERE record_kind='post' AND record_id=?")
        .bind(raw.to_string())
        .bind(post.as_str())
        .execute(&mut observer)
        .await?;
    let replay = fixture.api.call("thread_create", request).await?;
    accepted(&replay);
    assert_eq!(
        replay.pointer("/result/reader/unreadCount"),
        Some(&json!(3))
    );
    let position = selected
        .envelope()
        .position
        .project_cursor()
        .ok_or("post position")?;
    let after = FeedCursor {
        epoch: position.epoch,
        position: FeedPosition::try_from(position.position.get() - 1)?,
    };
    let content = fixture.api.call("feed_read", json!({"projectId":project,"reader":{"kind":"anonymous"},"after":after,"limit":1,"includeRedacted":true})).await?;
    assert_eq!(
        content.pointer("/error/data/kind"),
        Some(&json!("invalidRecord")),
        "{content}"
    );
    observer.close().await?;
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn scoped_thread_metadata_keeps_off_subject_epoch_and_dense_position_checks()
-> Result<(), TestError> {
    for epoch_fault in [true, false] {
        let fixture = BoardFixture::start().await?;
        let (project, lane) = fixture.project().await?;
        let (request, reply) = create(
            &fixture,
            &project,
            &PostId::generate(),
            &lane,
            "line-check reader",
        )
        .await?;
        accepted(&reply);
        let store = fixture
            ._registry
            .existing_project(&project)
            .await?
            .ok_or("project")?;
        let mut observer = sqlx_turso::TursoConnectOptions::new()
            .filename(store.path())
            .foreign_keys(false)
            .connect()
            .await?;
        let statement = if epoch_fault {
            "UPDATE project_feed SET epoch=2 WHERE record_kind='workstream' AND record_id=?"
        } else {
            "DELETE FROM project_feed WHERE record_kind='workstream' AND record_id=?"
        };
        sqlx::query(statement)
            .bind(lane.as_str())
            .execute(&mut observer)
            .await?;
        let replay = fixture.api.call("thread_create", request).await?;
        assert_eq!(
            replay.pointer("/error/data/kind"),
            Some(&json!("boardUnavailable")),
            "{replay}"
        );
        observer.close().await?;
        fixture.stop().await?;
    }
    Ok(())
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn scoped_native_selection_retains_every_thread_fact_and_intent_branch()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, lane) = fixture.project().await?;
    let post = PostId::generate();
    let thread = ThreadId::from(post.clone());
    let (request, reply) = create(&fixture, &project, &post, &lane, "selected kinds").await?;
    accepted(&reply);
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    let Some(Record::Project(state)) = read.record(&project.clone().into()).await? else {
        return Err("project state".into());
    };
    let mut envelope = state.envelope;
    envelope.request = None;
    read.finish().await?;
    let linked = FactId::generate();
    let evaluator = FactId::generate();
    let lifecycle = FactId::generate();
    let intent = NotifyIntentId::generate();
    let mut write = store.begin_admission(fixture.record_origin()?).await?;
    for (id, kind, links) in [
        (
            linked.clone(),
            FactKind::ScopeAcknowledgement {
                session: serde_json::from_value(
                    json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"read selector fixture"}),
                )?,
                subject: TrailSubject::Project,
                paths: vec![],
                reason: "read selector".to_owned(),
            },
            vec![project.clone().into(), thread.clone().into()],
        ),
        (
            evaluator.clone(),
            FactKind::EvaluatorResult {
                guard: "read selector".to_owned(),
                subject: thread.clone().into(),
                outcome: GuardOutcome::Met,
                evidence: vec![],
                evaluator: EvaluatorRef {
                    actor: envelope.actor.clone(),
                    engine: None,
                    calibration: None,
                },
                executor: None,
            },
            vec![project.clone().into()],
        ),
        (
            lifecycle.clone(),
            FactKind::Lifecycle(LifecycleFact::ThreadResolved {
                thread: thread.clone(),
                reason: ThreadResolutionReason::try_from("read selector".to_owned())?,
            }),
            vec![],
        ),
    ] {
        write
            .append_record(Record::Fact(Fact {
                envelope: RecordEnvelope {
                    id: id.clone().into(),
                    ..envelope.clone()
                },
                fact_id: id,
                kind,
                links,
                producer_time: chrono::Utc::now(),
                anchor: None,
                late: false,
            }))
            .await?;
    }
    write
        .append_record(Record::NotifyIntent(NotifyIntent::new(
            NotifyIntentProps {
                envelope: RecordEnvelope {
                    id: intent.clone().into(),
                    ..envelope.clone()
                },
                intent_id: intent.clone(),
                cause: thread.clone().into(),
                recipient: IntentRecipient::Resolved(ResolvedRecipient {
                    actor: envelope.actor,
                    via_role: None,
                }),
                state: IntentState::Emitted {},
            },
        )?))
        .await?;
    write.commit().await?;
    let mut read = store.snapshot().await?;
    let entries = read
        .unread_state_after(
            &thread.clone().into(),
            FeedCursor {
                epoch: read.high_water().epoch,
                position: FeedPosition::ZERO,
            },
        )
        .await?;
    assert_eq!(entries.unread_count, 7);
    let references = entries.unread;
    let ledger: RequestId =
        serde_json::from_value(request.get("requestId").ok_or("request id")?.clone())?;
    assert_eq!(
        references,
        vec![
            intent.into(),
            lifecycle.into(),
            evaluator.into(),
            linked.into(),
            ledger.into(),
            post.into(),
            thread.into()
        ]
    );
    read.finish().await?;
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn write_reader_counts_all_unread_but_returns_newest_fifty_and_pages_the_rest()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, lane) = fixture.project().await?;
    let post = PostId::generate();
    let thread = ThreadId::from(post.clone());
    let (request, reply) = create(&fixture, &project, &post, &lane, "bounded metadata").await?;
    accepted(&reply);
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    let Some(Record::Post(template)) = read.record(&post.clone().into()).await? else {
        return Err("post".into());
    };
    let creating_request = template
        .envelope
        .request
        .clone()
        .ok_or("creating request")?;
    let zero = FeedCursor {
        epoch: read.high_water().epoch,
        position: FeedPosition::ZERO,
    };
    read.finish().await?;
    let mut write = store.begin_admission(fixture.record_origin()?).await?;
    let mut newest = Vec::new();
    for number in 0..60 {
        let id = PostId::generate();
        newest.push(RecordRef::from(id.clone()));
        write
            .append_record(Record::Post(Post {
                envelope: RecordEnvelope {
                    id: id.clone().into(),
                    request: None,
                    ..template.envelope.clone()
                },
                post_id: id,
                placement: Placement::Thread {
                    thread_id: thread.clone(),
                },
                text: PostText::try_from(format!("historical reply {number}"))?,
                ..template.clone()
            }))
            .await?;
    }
    write.commit().await?;
    let replay = fixture.api.call("thread_create", request).await?;
    accepted(&replay);
    assert_eq!(
        replay.pointer("/result/reader/unreadCount"),
        Some(&json!(63)),
        "{replay}"
    );
    let unread: Vec<RecordRef> = serde_json::from_value(
        replay
            .pointer("/result/reader/unread")
            .ok_or("unread")?
            .clone(),
    )?;
    assert_eq!(
        unread,
        newest.into_iter().rev().take(50).collect::<Vec<_>>()
    );
    let mut after = zero;
    let mut paged = Vec::new();
    loop {
        let mut read = store.snapshot().await?;
        let FeedRead::Entries { entries, .. } = read.feed_after(after, 10, true).await? else {
            return Err("feed page".into());
        };
        read.finish().await?;
        if entries.is_empty() {
            break;
        }
        for entry in entries {
            after = FeedCursor {
                epoch: entry.epoch,
                position: entry.position,
            };
            let selected = entry.record == RecordRef::from(thread.clone())
                || match &entry.content {
                    FeedContent::Shown(record) => match record.as_ref() {
                        Record::Post(row) => {
                            row.post_id == post
                                || matches!(&row.placement, Placement::Thread { thread_id } if thread_id == &thread)
                        }
                        Record::Request(row) => {
                            matches!(&row.decision, StoredDecision::Accepted { .. })
                                && row.request_id == creating_request
                        }
                        _ => false,
                    },
                    _ => false,
                };
            if selected {
                paged.push(entry.record);
            }
        }
    }
    assert_eq!(paged.len(), 63);
    assert_eq!(unread, paged.into_iter().rev().take(50).collect::<Vec<_>>());
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn task_workstream_and_project_metadata_follow_subject_relations_without_decoding_titles()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let task = fixture.task(&project, &workstream).await?;
    let child = SubtaskId::generate();
    accepted(&fixture.api.call("subtask_add", fixture.request(&project, json!({"kind":"subtaskAdd","subtaskId":child,"taskId":task,"text":"metadata checklist","place":"80"}))).await?);
    let task_trail = WorkTrailRowId::generate();
    let stream_trail = WorkTrailRowId::generate();
    for (row, subject) in [
        (task_trail.clone(), json!({"kind":"task","subject":task})),
        (
            stream_trail.clone(),
            json!({"kind":"workstream","subject":workstream}),
        ),
    ] {
        accepted(&fixture.api.call("work_trail_add", fixture.request(&project, json!({"kind":"workTrailAdd","rowId":row,"subject":subject,"text":"metadata evidence","tags":[],"links":[]}))).await?);
    }
    let role = fixture.api.call("role_grant", fixture.request(&project, json!({"kind":"roleGrant","subject":{"kind":"workstream","subject":workstream},"role":"implementer","holder":{"kind":"human","humanId":"metadata implementer"},"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}))).await?;
    accepted(&role);
    let assignment: RecordRef = serde_json::from_value(
        role.pointer("/result/result/decision/effects/entries/0/0")
            .ok_or("assignment")?
            .clone(),
    )?;
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    let zero = FeedCursor {
        epoch: read.high_water().epoch,
        position: FeedPosition::ZERO,
    };
    let task_metadata = read.unread_state_after(&task.clone().into(), zero).await?;
    assert_eq!(task_metadata.unread_count, 4);
    assert!(task_metadata.unread.contains(&task.clone().into()));
    assert!(task_metadata.unread.contains(&child.clone().into()));
    assert!(task_metadata.unread.contains(&task_trail.clone().into()));
    assert!(!task_metadata.unread.contains(&stream_trail.clone().into()));
    let stream_metadata = read
        .unread_state_after(&workstream.clone().into(), zero)
        .await?;
    assert_eq!(stream_metadata.unread_count, 7);
    assert!(stream_metadata.unread.contains(&assignment));
    let trail_metadata = read
        .unread_state_after(&task_trail.clone().into(), zero)
        .await?;
    assert_eq!(trail_metadata.unread_count, 2);
    assert!(trail_metadata.unread.contains(&task_trail.clone().into()));
    for reference in [
        workstream.clone().into(),
        task.clone().into(),
        child.into(),
        task_trail.into(),
        stream_trail.into(),
    ] {
        assert!(stream_metadata.unread.contains(&reference));
    }
    let project_metadata = read
        .unread_state_after(&project.clone().into(), zero)
        .await?;
    assert_eq!(
        project_metadata.unread_count,
        read.high_water().position.get()
    );
    let tail = read
        .unread_state_after(&project.clone().into(), read.high_water())
        .await?;
    assert_eq!(tail.unread_count, 0);
    assert!(tail.unread.is_empty());
    read.finish().await?;
    let mut observer = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .connect()
        .await?;
    sqlx::query("UPDATE project_feed SET content=json_set(content, '$.record.record.title', ' padded stored title ') WHERE record_kind='task' AND record_id=?").bind(task.as_str()).execute(&mut observer).await?;
    let mut read = store.snapshot().await?;
    assert_eq!(
        read.unread_state_after(&workstream.into(), zero).await?,
        stream_metadata
    );
    assert_eq!(
        read.unread_state_after(&task.into(), zero).await?,
        task_metadata
    );
    assert_eq!(
        read.unread_state_after(&project.into(), zero).await?,
        project_metadata
    );
    assert!(matches!(
        read.feed_after(zero, u64::MAX, true).await,
        Err(message_board_storage::ProjectStorageError::InvalidRecord { field: "title", .. })
    ));
    read.finish().await?;
    observer.close().await?;
    fixture.stop().await
}
