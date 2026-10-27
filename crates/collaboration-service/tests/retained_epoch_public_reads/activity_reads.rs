//! Retained activity and trail reads use row epochs, including after physical reopen.
use super::{
    accepted_snapshot, append_project_revision, board_fixture, board_read_setup, post_cursor,
};
use board_fixture::{BoardFixture, TestError, accepted};
use board_read_setup::{human_reader, inbox_post_ids, inbox_request, post_at_placement};
use collaboration_mcp::test_support::ServedCollaborationApi;
use collaboration_protocol::{
    EndpointRef, MessageShowResult, PushActivityRange, PushActivitySnapshot, PushHeaderFacts,
    PushId, PushKind, PushOrigin, PushRecordDraft, PushRecordShowParams, RouterOriginRef,
    SessionRef as RouterSessionRef,
};
use collaboration_service::ApplicationFixture;
use message_board::*;
use serde_json::json;
use std::{os::unix::fs::PermissionsExt, sync::Arc};

const SERVICE: &str = "00000000-0000-4000-8000-000000000001";

async fn push_api(
    fixture: &BoardFixture,
    pushes: Arc<interaction_storage::InteractionStore>,
    directory: &std::path::Path,
) -> Result<ServedCollaborationApi, TestError> {
    tokio::fs::create_dir_all(directory).await?;
    std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))?;
    Ok(ServedCollaborationApi::start(
        directory,
        (ApplicationFixture::new(SERVICE, "00000000-0000-4000-8000-000000000002")?
            .with_project_admission(fixture._admission.clone())
            .with_interaction_store(pushes))
        .build(),
    )?)
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture and transport failures."
)]
async fn retained_trail_search_inbox_and_stored_push_ranges_survive_a_later_writer_epoch()
-> Result<(), TestError> {
    let source = BoardFixture::start().await?;
    let (project, _) = source.project().await?;
    let root = post_at_placement(
        &source,
        Placement::Project {
            project_id: project.clone(),
        },
        json!({"text":"retained root"}),
    )
    .await?;
    let thread = ThreadId::from(root.clone());
    let old_reply = post_at_placement(
        &source,
        Placement::Thread {
            thread_id: thread.clone(),
        },
        json!({"text":"retained reply"}),
    )
    .await?;
    let old_cursor = post_cursor(&source, &project, &old_reply).await?;
    let inbox_reader = human_reader("retained inbox reader")?;
    accepted(
        &board_read_setup::keyed_call(
            &source,
            "thread_watch",
            json!({
                "actor":inbox_reader,"thread":{"thread":thread},
            }),
        )
        .await?,
    );
    let mut rows = Vec::new();
    for text in ["first retained decision", "second retained decision"] {
        let row = WorkTrailRowId::generate();
        accepted(
            &source
                .api
                .call(
                    "work_trail_add",
                    source.request(
                        &project,
                        json!({
                            "kind":"workTrailAdd","rowId":row,"subject":{"kind":"project"},
                            "text":text,"tags":["decision"],"links":[],
                        }),
                    ),
                )
                .await?,
        );
        let store = source
            ._registry
            .existing_project(&project)
            .await?
            .ok_or("source")?;
        let mut read = store.snapshot().await?;
        let record = read.record(&row.clone().into()).await?.ok_or("trail row")?;
        rows.push((
            row,
            record
                .envelope()
                .position
                .project_cursor()
                .ok_or("row cursor")?,
        ));
        read.finish().await?;
    }
    let mut target = BoardFixture::start().await?;
    target.workspace = source.workspace.clone();
    target.board = source.board.clone();
    // Snapshot transfer is real; the directory policy answer is the same
    // explicit fixture input used by BoardFixture's registered creation.
    for (tool, fields) in [
        (
            "workspace_create",
            json!({"workspaceId":source.workspace,"name":"copied scope","description":""}),
        ),
        (
            "board_create",
            json!({"workspaceId":source.workspace,"boardId":source.board,"name":"copied board","description":"","kind":{"kind":"area"}}),
        ),
    ] {
        accepted(&board_read_setup::keyed_call(&target, tool, fields).await?);
    }
    let mut creating = target.header();
    creating.as_object_mut().ok_or("creating input")?.extend(
        json!({"projectId":project,"boardId":source.board,"title":"copied project"})
            .as_object()
            .ok_or("creating fields")?
            .clone(),
    );
    board_fixture::settle_accepted_creation(
        &target._fleet,
        &serde_json::from_value(creating)?,
        &target.record_origin()?,
    )
    .await?;
    let (_, epoch) = accepted_snapshot::bootstrap_project(&source, &target, &project).await?;
    let new_head = append_project_revision(&target, &project).await?;
    assert_eq!(new_head.epoch, epoch);
    let client = target.api.client("retained activity reads").await?;

    let mut search = PostSearchRequest {
        scope: PostListScope::Project {
            project_id: project.clone(),
        },
        query: "retained".to_owned().try_into()?,
        kind: SearchPostKind::Both,
        include_archived: true,
        page: PageRequest {
            limit: PageLimit::try_from(1)?,
            cursor: None,
        },
    };
    let first = client.post_search(search.clone()).await?.page;
    assert_eq!(
        first.records.first().map(|located| &located.post.post_id),
        Some(&old_reply)
    );
    search.page.cursor = Some(first.next_cursor.ok_or("search continuation")?);
    let second = client.post_search(search).await?.page;
    assert_eq!(
        second.records.first().map(|located| &located.post.post_id),
        Some(&root)
    );
    assert!(second.next_cursor.is_none());

    let mut inbox = inbox_request(
        InboxScope::Project {
            project_id: project.clone(),
        },
        inbox_reader,
        InboxReadMode::Latest,
    )?;
    inbox.page.limit = PageLimit::try_from(1)?;
    let first = client.inbox_fetch(inbox.clone()).await?.page;
    assert_eq!(inbox_post_ids(&first.records), vec![old_reply.clone()]);
    assert_eq!(first.next.per_project.get(&project), Some(&old_cursor));
    inbox.page.cursor = Some(first.next_cursor.ok_or("inbox continuation")?);
    let second = client.inbox_fetch(inbox).await?.page;
    assert_eq!(inbox_post_ids(&second.records), vec![root]);

    let reader = ReadActor::Identified {
        reader: human_reader("retained trail reader")?,
    };
    let store = target
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("target")?;
    let mut read = store.snapshot().await?;
    let (first_row, first_cursor) = rows.first().ok_or("first decision")?;
    let after = read
        .feed_cursor_or_start(FeedPosition::try_from(first_cursor.position.get() - 1)?)
        .await?;
    read.finish().await?;
    let result = client
        .work_trail_show(WorkTrailShowRequest {
            project_id: project.clone(),
            reader: reader.clone(),
            selection: WorkTrailSelection::Forward { after },
            limit: PageLimit::try_from(1)?,
            subject: None,
            tags: Vec::new(),
            include_redacted: false,
        })
        .await?;
    let WorkTrailReadResult::Forward(forward) = result.value else {
        return Err("retained trail was reported superseded".into());
    };
    assert_eq!(forward.rows.first().map(|row| &row.row_id), Some(first_row));
    assert_eq!(forward.covered_through, *first_cursor);
    assert_eq!(forward.high_water, new_head);
    assert_eq!(
        result.read_mark,
        Some(ReadMarkOutcome::Advanced {
            through: *first_cursor
        })
    );
    let mut decisions = WorkTrailShowRequest {
        project_id: project.clone(),
        reader,
        selection: WorkTrailSelection::Decisions {
            after: None,
            before: None,
            range_top: None,
        },
        limit: PageLimit::try_from(1)?,
        subject: None,
        tags: Vec::new(),
        include_redacted: false,
    };
    let WorkTrailReadResult::Decisions(first) =
        client.work_trail_show(decisions.clone()).await?.value
    else {
        return Err("decision page".into());
    };
    assert_eq!(
        first.rows.first().map(|row| &row.row_id),
        rows.last().map(|(id, _)| id)
    );
    let older = first.older_than.ok_or("old assigning epoch continuation")?;
    assert_eq!(older.epoch, old_cursor.epoch);
    assert_eq!(first.range_top, new_head);
    decisions.selection = WorkTrailSelection::Decisions {
        after: None,
        before: Some(older),
        range_top: Some(first.range_top),
    };
    let WorkTrailReadResult::Decisions(second) = client.work_trail_show(decisions).await?.value
    else {
        return Err("continued decision page".into());
    };
    assert_eq!(second.rows.first().map(|row| &row.row_id), Some(first_row));
    assert!(second.older_than.is_none());
    assert_eq!(second.covered_through, Some(new_head));
    drop(store);
    drop(client);

    let new_reply = post_at_placement(
        &target,
        Placement::Thread {
            thread_id: thread.clone(),
        },
        json!({"text":"retained new epoch reply"}),
    )
    .await?;
    let new_cursor = post_cursor(&target, &project, &new_reply).await?;
    assert_eq!(new_cursor.epoch, epoch);
    let discarded = post_at_placement(
        &source,
        Placement::Thread {
            thread_id: thread.clone(),
        },
        json!({"text":"old writer suffix excluded from the accepted snapshot"}),
    )
    .await?;
    let discarded = post_cursor(&source, &project, &discarded).await?;
    assert!(discarded.position <= new_cursor.position);
    let store = target
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("current target")?;
    let read = store.snapshot().await?;
    let future = FeedCursor {
        position: read.high_water().position.successor()?,
        epoch,
    };
    read.finish().await?;
    drop(store);
    let target_session = RouterSessionRef {
        endpoint: EndpointRef {
            service_id: SERVICE.to_owned().try_into()?,
            endpoint_id: "codex-local".to_owned().try_into()?,
        },
        session_id: "retained-push-reader".to_owned().try_into()?,
    };
    let path = target._directory.path().join("retained-interaction.sqlite");
    let pushes = Arc::new(interaction_storage::InteractionStore::open(&path).await?);
    let mut expected = Vec::new();
    for (from, through, ids) in [
        (old_cursor, old_cursor, Some(vec![old_reply.clone()])),
        (
            old_cursor,
            new_cursor,
            Some(vec![old_reply.clone(), new_reply.clone()]),
        ),
        (discarded, new_cursor, None),
        (
            old_cursor,
            FeedCursor {
                epoch: EpochNumber::try_from(99)?,
                ..new_cursor
            },
            None,
        ),
        (old_cursor, future, None),
    ] {
        let push_id = PushId::try_from(uuid::Uuid::now_v7().to_string())?;
        let origin = RouterOriginRef::SubscriptionActivity {
            target: target_session.clone(),
            batch_id: BatchId::generate(),
        };
        pushes
            .insert_push_record(PushRecordDraft {
                push_id: push_id.clone(),
                kind: PushKind::SubscriptionActivity,
                origin: PushOrigin::Router(PushKind::SubscriptionActivity),
                origin_router_ref: Some(origin.canonical_string()?),
                target: target_session.clone(),
                reply_to_push_id: None,
                header_facts: PushHeaderFacts::SubscriptionActivity {
                    root_count: 1,
                    message_count: u64::try_from(ids.as_ref().map_or(1, Vec::len))?,
                    held_since: None,
                    thread_resolved: false,
                },
                body: None,
                activity: Some(PushActivitySnapshot {
                    ranges: vec![PushActivityRange {
                        project_id: project.clone(),
                        thread_id: thread.clone(),
                        from,
                        through,
                    }],
                    held: false,
                    draining: false,
                }),
                mode: None,
                guard: None,
                created_at: chrono::Utc::now(),
            })
            .await?;
        expected.push((push_id, ids));
    }
    Arc::try_unwrap(pushes)
        .map_err(|_| "push owner")?
        .close()
        .await?;
    let target = target.restart_read_fixture().await?;
    let pushes = Arc::new(interaction_storage::InteractionStore::open(path).await?);
    let api = push_api(
        &target,
        pushes.clone(),
        &target._directory.path().join("range-reader"),
    )
    .await?;
    let client = api.client("retained push range reopen").await?;
    for (push_id, ids) in expected {
        let shown = client
            .router_show(PushRecordShowParams {
                caller: target_session.clone(),
                reference: push_id.as_str().to_owned(),
            })
            .await;
        let Some(ids) = ids else {
            assert!(
                shown.is_err(),
                "invalid stored range read replacement rows: {push_id:?}"
            );
            continue;
        };
        let shown = shown?;
        let MessageShowResult::Local(shown) = shown else {
            return Err("local range reply".into());
        };
        assert_eq!(shown.activity_ranges.len(), 1);
        let range = shown.activity_ranges.first().ok_or("expanded range")?;
        assert_eq!(
            range
                .posts
                .iter()
                .map(|post| post.post_id.clone())
                .collect::<Vec<_>>(),
            ids
        );
        assert!(
            shown.record.read_at.is_none(),
            "expansion must not acknowledge the subscription"
        );
        assert!(
            pushes
                .get_push_record(&push_id)
                .await?
                .ok_or("stored push")?
                .read_at
                .is_none()
        );
    }
    drop(client);
    api.stop().await?;
    Arc::try_unwrap(pushes)
        .map_err(|_| "reopened push owner")?
        .close()
        .await?;
    target.stop().await?;
    source.stop().await
}
