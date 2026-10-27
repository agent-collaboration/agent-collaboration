#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! Retained public SDK journeys on separate native project and fleet stores.
use collaboration_client::board::RequestResult;
use message_board::*;
use serde_json::json;
#[path = "support/board_fixture.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
#[expect(
    clippy::indexing_slicing,
    reason = "This test consumes a subset of the shared fixture and retains its fixed JSON construction."
)]
pub mod board_fixture;
#[path = "board_control_support/validation.rs"]
mod validation;
use board_fixture::TestError;

#[path = "board_control_support/fixture_setup.rs"]
mod fixture_setup;
use fixture_setup::{
    accepted, acknowledge, cursor, inbox_request, post, refused, request, session, start,
};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove the contract; Result propagates fixture errors."
)]
async fn board_control_roundtrip_preserves_root_thread_and_actor() -> Result<(), TestError> {
    let (fixture, client, project) = start().await?;
    let actor: Identity = serde_json::from_value(fixture.actor.clone())?;
    let root = post(
        &fixture,
        &client,
        &actor,
        Placement::Project {
            project_id: project.clone(),
        },
        "Main",
        vec![],
    )
    .await?;
    let thread = ThreadId::from(root.post_id.clone());
    let shown = client
        .thread_show(ThreadShowRequest {
            include_redacted: false,
            thread: (thread.clone()).into(),
            reader: ReadActor::Identified {
                reader: actor.clone(),
            },
        })
        .await?;
    assert!(
        !shown
            .watch_status
            .as_ref()
            .ok_or("watch status missing")?
            .watching
    );
    let reply = post(
        &fixture,
        &client,
        &actor,
        Placement::Thread {
            thread_id: thread.clone(),
        },
        "Thread",
        vec![ReferenceTarget::Post {
            post_id: root.post_id.clone(),
        }],
    )
    .await?;
    assert_eq!(reply.actor, actor);
    let latest = client
        .inbox_fetch(inbox_request(&actor, &project, InboxReadMode::Latest)?)
        .await?
        .page;
    assert_eq!(latest.read_mode, InboxReadMode::Latest);
    assert_eq!(latest.ordering, PostOrdering::NewestFirst);
    assert!(latest.initializations.is_empty());
    assert_eq!(
        latest.records.len(),
        1,
        "latest retains unwatched thread context"
    );
    assert!(
        matches!(&latest.records[0],InboxActivity::PostCreated { post, .. } if post.post_id == root.post_id)
    );
    let discovery = client
        .discovery_search(DiscoverySearchRequest {
            query: SearchQuery::try_from("project".to_owned())?,
            scope: DiscoveryScope::Board {
                board_id: fixture.board.clone(),
            },
            kind: DiscoveryKind::ProjectDirectoryEntry,
            include_archived: false,
            page: PageRequest::default(),
        })
        .await?;
    assert_eq!(discovery.page.records.len(), 1);
    let search = client
        .post_search(PostSearchRequest {
            query: SearchQuery::try_from("THREAD".to_owned())?,
            scope: PostListScope::Thread {
                thread: (thread.clone()).into(),
            },
            kind: SearchPostKind::Thread,
            include_archived: false,
            page: PageRequest::default(),
        })
        .await?;
    assert_eq!(search.page.records.len(), 1);
    assert_eq!(search.page.records[0].post.post_id, reply.post_id);
    assert_eq!(
        client
            .thread_show(ThreadShowRequest {
                include_redacted: false,
                thread: (thread).into(),
                reader: ReadActor::Anonymous {}
            })
            .await?
            .thread
            .state,
        ThreadState::Open
    );
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove the contract; Result propagates fixture errors."
)]
async fn control_thread_list_pages_escape_heavy_holders_without_skips_or_repeats()
-> Result<(), TestError> {
    let (fixture, client, project) = start().await?;
    let owner: Identity = serde_json::from_value(fixture.actor.clone())?;
    // The subject stays fixed while holder identities and admitted writes vary.
    let discussion_subject = fixture.discussion_subject(&project).await?;
    let mut expected = Vec::new();
    let mut write_latencies = Vec::new();
    let creation_started = std::time::Instant::now();
    for index in 0..100 {
        let prefix = format!("{index:04}");
        let actor = session(format!("{prefix}{}", "\u{1}".repeat(4_096 - prefix.len())))?;
        let post_id = PostId::generate();
        let write_started = std::time::Instant::now();
        accepted(&client.thread_create(request(&actor,json!({"postId":post_id,"projectId":project,"about":discussion_subject,"text":"Root","references":[],"addressees":[],"role":"orchestrator","watch":false}))?).await?);
        write_latencies.push(write_started.elapsed().as_micros());
        expected.push(ThreadId::from(post_id));
    }
    let creation_elapsed = creation_started.elapsed();
    write_latencies.sort_unstable();
    eprintln!(
        "100 maximum escaped-holder admitted writes: p50={}us; p95={}us; max={}us",
        write_latencies.get(49).ok_or("p50")?,
        write_latencies.get(94).ok_or("p95")?,
        write_latencies.last().ok_or("max")?
    );
    if !cfg!(debug_assertions) {
        assert!(
            *write_latencies.get(94).ok_or("p95")? < 50_000,
            "release maximum-identity admitted write p95 must stay below 50ms"
        );
    }
    expected.sort();
    let mut next_cursor = None;
    let mut observed = Vec::new();
    let traversal_started = std::time::Instant::now();
    loop {
        let result = client
            .thread_list(ThreadListRequest {
                workspace_id: fixture.workspace.clone(),
                reader: owner.clone(),
                watched_only: false,
                page: PageRequest {
                    limit: PageLimit::try_from(100)?,
                    cursor: next_cursor,
                },
            })
            .await?;
        let response = json!({"jsonrpc":"2.0","id":"thread-list-frame-budget","result":result});
        assert!(
            serde_json::to_vec(&response)?.len() < collaboration_protocol::MAX_CONTROL_FRAME_BYTES
        );
        let result: ThreadListResult = serde_json::from_value(response["result"].clone())?;
        assert!(
            result
                .page
                .records
                .iter()
                .all(|thread| thread.thread.orchestrator.is_some())
        );
        observed.extend(
            result
                .page
                .records
                .into_iter()
                .map(|thread| thread.thread.thread_id),
        );
        next_cursor = result.page.next_cursor;
        if next_cursor.is_none() {
            break;
        }
    }
    assert_eq!(
        observed, expected,
        "thread cursor traversal skipped or repeated a holder"
    );
    let traversal_elapsed = traversal_started.elapsed();
    if !cfg!(debug_assertions) {
        assert!(
            traversal_elapsed <= std::time::Duration::from_secs(1),
            "release complete 100-holder traversal must stay within 1s; observed {traversal_elapsed:?}"
        );
    }
    let shutdown_started = std::time::Instant::now();
    fixture.stop().await?;
    eprintln!(
        "100 escape-heavy holders: public creation={}us; complete traversal={}us; joined shutdown={}us",
        creation_elapsed.as_micros(),
        traversal_elapsed.as_micros(),
        shutdown_started.elapsed().as_micros()
    );
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Measure normal UUID identities separately; the permanent maximum-identity frame proof remains unchanged."
)]
async fn control_uuid_holder_writes_report_scope_growth_latency() -> Result<(), TestError> {
    let (fixture, client, project) = start().await?;
    let owner: Identity = serde_json::from_value(fixture.actor.clone())?;
    let mut expected = Vec::new();
    let mut write_latencies = Vec::new();
    for _ in 0..100 {
        let actor = session(PostId::generate().as_str().to_owned())?;
        let post = PostId::generate();
        let started = std::time::Instant::now();
        accepted(&client.thread_create(request(&actor, json!({"postId":post,"projectId":project,"about":fixture.discussion_subject(&project).await?,"text":"Root","references":[],"addressees":[],"role":"orchestrator","watch":false}))?).await?);
        write_latencies.push(started.elapsed().as_micros());
        expected.push(ThreadId::from(post));
    }
    let started = std::time::Instant::now();
    let threads = client
        .thread_list(ThreadListRequest {
            workspace_id: fixture.workspace.clone(),
            reader: owner,
            watched_only: false,
            page: PageRequest {
                limit: PageLimit::try_from(100)?,
                cursor: None,
            },
        })
        .await?;
    assert_eq!(threads.page.records.len(), 100);
    assert!(threads.page.next_cursor.is_none());
    assert!(
        threads
            .page
            .records
            .iter()
            .all(|thread| thread.thread.orchestrator.is_some())
    );
    expected.sort();
    assert_eq!(
        threads
            .page
            .records
            .into_iter()
            .map(|thread| thread.thread.thread_id)
            .collect::<Vec<_>>(),
        expected
    );
    write_latencies.sort_unstable();
    eprintln!(
        "100 UUID holders: admitted write p50={}us; p95={}us; max={}us; traversal={}us",
        write_latencies.get(49).ok_or("p50")?,
        write_latencies.get(94).ok_or("p95")?,
        write_latencies.last().ok_or("max")?,
        started.elapsed().as_micros()
    );
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove the contract; Result propagates fixture errors."
)]
async fn control_board_failures_pagination_and_inbox_use_the_public_path() -> Result<(), TestError>
{
    let (fixture, client, project) = start().await?;
    let alice: Identity = serde_json::from_value(fixture.actor.clone())?;
    let bob = Identity::Human {
        human_id: HumanId::try_from("bob".to_owned())?,
    };
    for (duplicate_project, refusal) in [
        (
            project.clone(),
            Refusal::AlreadyExists(project.clone().into()),
        ),
        (
            ProjectId::generate(),
            Refusal::NameConflict {
                name: ResourceName::try_from("Project".to_owned())?,
                existing: project.clone().into(),
            },
        ),
    ] {
        let create: ProjectCreateRequest = request(
            &alice,
            json!({"projectId":duplicate_project,"boardId":fixture.board,"title":"Project"}),
        )?;
        // The connected central decides the collision; the SDK must preserve
        // that exact refusal on both the initial call and keyed replay.
        let rejected = client.project_create(create.clone()).await?;
        assert_eq!(
            rejected.result,
            RequestResult::ScopeUnavailable(ScopeRefusal::ProjectRejected {
                project: duplicate_project,
                reason: ProjectRejection::Directory { refusal },
            })
        );
        assert_eq!(client.project_create(create).await?.result, rejected.result);
    }
    let mut initialize = inbox_request(&bob, &project, InboxReadMode::Unread)?;
    initialize = InboxFetchRequest::new(InboxFetchRequestProps {
        initializations: vec![ReadInitializationEntry {
            project_id: project.clone(),
            request_id: RequestId::generate(),
        }],
        ..initialize.into()
    })?;
    let initialized = client.inbox_fetch(initialize).await?;
    let ProjectWriteCallResult::Replied { reply } = &initialized.page.initializations[0].outcome
    else {
        return Err("initializer failed".into());
    };
    accepted(reply);
    let root = post(
        &fixture,
        &client,
        &alice,
        Placement::Project {
            project_id: project.clone(),
        },
        "Root",
        vec![],
    )
    .await?;
    let thread = ThreadId::from(root.post_id.clone());
    accepted(
        &client
            .thread_watch(request(&bob, json!({"thread":{"thread":thread}}))?)
            .await?,
    );
    let reply = post(
        &fixture,
        &client,
        &alice,
        Placement::Thread {
            thread_id: thread.clone(),
        },
        "Reply",
        vec![ReferenceTarget::Post {
            post_id: root.post_id.clone(),
        }],
    )
    .await?;
    let inbox = client
        .inbox_fetch(inbox_request(&bob, &project, InboxReadMode::Unread)?)
        .await?
        .page;
    assert!(inbox.records.iter().any(|activity|matches!(activity,InboxActivity::PostCreated { post,.. } if post.post_id==reply.post_id)));
    acknowledge(
        &client,
        &bob,
        ReadScope::Thread {
            thread_id: thread.clone(),
        },
        cursor(&reply)?,
    )
    .await?;
    assert!(
        !client
            .inbox_fetch(inbox_request(&bob, &project, InboxReadMode::Unread)?)
            .await?
            .page
            .records
            .is_empty(),
        "thread ack cleared independent project root"
    );
    acknowledge(
        &client,
        &bob,
        ReadScope::Project {
            project_id: project.clone(),
        },
        cursor(&root)?,
    )
    .await?;
    assert!(
        client
            .inbox_fetch(inbox_request(&bob, &project, InboxReadMode::Unread)?)
            .await?
            .page
            .records
            .is_empty()
    );
    accepted(
        &client
            .thread_resolve(request(
                &alice,
                json!({"thread":{"thread":thread},"reason":"conversation complete"}),
            )?)
            .await?,
    );
    let resolved=client.thread_post(request(&bob,json!({"postId":PostId::generate(),"placement":{"kind":"thread","thread":{"thread":thread}},"text":"Blocked","references":[],"addressees":[]}))?).await?;
    assert!(refused(&resolved)?.iter().any(|reason|matches!(reason,Refusal::Closed { subject } if subject==&RecordRef::from(thread.clone()))));
    accepted(
        &client
            .thread_unresolve(request(&alice, json!({"thread":{"thread":thread}}))?)
            .await?,
    );
    let escaped_body = "\\\"".repeat(32_768);
    for _ in 0..9 {
        post(
            &fixture,
            &client,
            &alice,
            Placement::Thread {
                thread_id: thread.clone(),
            },
            &escaped_body,
            vec![],
        )
        .await?;
    }
    let mut next_cursor = None;
    let mut observed = 0;
    let mut pages = 0;
    loop {
        let result = client
            .post_list(PostListRequest {
                scope: PostListScope::Board {
                    board_id: fixture.board.clone(),
                },
                selection: PostSelection::Latest,
                page: PageRequest {
                    limit: PageLimit::try_from(100)?,
                    cursor: next_cursor,
                },
            })
            .await?;
        assert!(
            serde_json::to_vec(&result)?.len() < collaboration_protocol::MAX_CONTROL_FRAME_BYTES,
            "post page exceeded frame budget"
        );
        observed += result.page.records.len();
        pages += 1;
        next_cursor = result.page.next_cursor;
        if next_cursor.is_none() {
            break;
        }
    }
    assert_eq!(observed, 11);
    assert!(pages >= 2, "byte budget did not split escaped content");
    accepted(
        &client
            .board_archive(request(&alice, json!({"boardId":fixture.board}))?)
            .await?,
    );
    let archived=client.thread_post(request(&alice,json!({"postId":PostId::generate(),"placement":{"kind":"thread","thread":{"thread":thread}},"text":"Archived","references":[],"addressees":[]}))?).await?;
    assert!(
        refused(&archived)?
            .iter()
            .any(|reason| matches!(reason, Refusal::Archived))
    );
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove the contract; Result propagates fixture errors."
)]
async fn control_participant_operations_preserve_typed_state_and_authorization()
-> Result<(), TestError> {
    let (fixture, client, project) = start().await?;
    let owner: Identity = serde_json::from_value(fixture.actor.clone())?;
    let orchestrator = session("orchestrator".to_owned())?;
    let reviewer = session("reviewer".to_owned())?;
    let unjoined = session("unjoined".to_owned())?;
    let post_id = PostId::generate();
    let thread = ThreadId::from(post_id.clone());
    accepted(&client.thread_create(request(&orchestrator,json!({"postId":post_id,"projectId":project,"about":fixture.discussion_subject(&project).await?,"text":"Root","references":[],"addressees":[],"role":"orchestrator","watch":true}))?).await?);
    assert_eq!(
        client
            .thread_show(ThreadShowRequest {
                include_redacted: false,
                thread: (thread.clone()).into(),
                reader: ReadActor::Anonymous {}
            })
            .await?
            .thread
            .orchestrator
            .as_ref()
            .map(|holder| &holder.identity),
        Some(&orchestrator)
    );
    let mut join = serde_json::to_value(request::<ThreadJoinRequest>(
        &reviewer,
        json!({"thread":{"thread":thread},"role":"reviewer","watch":true,"replace":null,"note":"Control reviewer"}),
    )?)?;
    join.as_object_mut().ok_or("join")?.remove("actingFor");
    accepted(&client.thread_join(serde_json::from_value(join)?).await?);
    let listed = client
        .thread_participant_list(ThreadParticipantListRequest {
            thread: (thread.clone()).into(),
            page: PageRequest::default(),
        })
        .await?;
    assert_eq!(listed.page.records.len(), 2);
    assert!(
        listed
            .page
            .records
            .iter()
            .any(|participant| participant.identity == reviewer && participant.is_open())
    );
    assert_eq!(
        listed.orchestrator.as_ref().map(|holder| &holder.identity),
        Some(&orchestrator)
    );
    assert!(
        client
            .thread_show(ThreadShowRequest {
                include_redacted: false,
                thread: (thread.clone()).into(),
                reader: ReadActor::Identified {
                    reader: reviewer.clone()
                }
            })
            .await?
            .watch_status
            .ok_or("watch status")?
            .watching
    );
    let unjoined_post=client.thread_post(request(&unjoined,json!({"postId":PostId::generate(),"placement":{"kind":"thread","thread":{"thread":thread}},"text":"Blocked","references":[],"addressees":[]}))?).await?;
    assert!(refused(&unjoined_post)?.iter().any(|reason|matches!(reason,Refusal::Thread(refusal) if matches!(refusal.as_ref(),ThreadRefusal::ParticipantRequired { thread_id,actor } if thread_id==&thread&&actor==&unjoined))));
    let session_resolve = client
        .thread_resolve(request(
            &reviewer,
            json!({"thread":{"thread":thread},"reason":"cannot close"}),
        )?)
        .await?;
    assert!(refused(&session_resolve)?.iter().any(|reason|matches!(reason,Refusal::Thread(refusal) if matches!(refusal.as_ref(),ThreadRefusal::OrchestratorRequired { thread_id,actor,holder,last_seen } if thread_id==&thread&&actor==&reviewer&&holder.as_ref()==Some(&orchestrator)&&last_seen.is_some()))));
    assert_eq!(
        client
            .thread_participant_list(ThreadParticipantListRequest {
                thread: (thread.clone()).into(),
                page: PageRequest::default()
            })
            .await?
            .page
            .records
            .len(),
        2
    );
    post(
        &fixture,
        &client,
        &owner,
        Placement::Thread {
            thread_id: thread.clone(),
        },
        "Human is exempt",
        vec![],
    )
    .await?;
    accepted(
        &client
            .thread_leave(request(
                &reviewer,
                json!({"thread":{"thread":thread},"afterLeave":null}),
            )?)
            .await?,
    );
    let listed = client
        .thread_participant_list(ThreadParticipantListRequest {
            thread: (thread.clone()).into(),
            page: PageRequest::default(),
        })
        .await?;
    assert!(
        listed
            .page
            .records
            .iter()
            .any(|participant| participant.identity == reviewer
                && participant.closed_reason == Some(ParticipantClosedReason::Left))
    );
    assert!(
        !client
            .thread_show(ThreadShowRequest {
                include_redacted: false,
                thread: (thread.clone()).into(),
                reader: ReadActor::Identified { reader: reviewer }
            })
            .await?
            .watch_status
            .ok_or("watch status")?
            .watching
    );
    accepted(
        &client
            .thread_resolve(request(
                &owner,
                json!({"thread":{"thread":thread},"reason":"owner closes"}),
            )?)
            .await?,
    );
    fixture.stop().await
}
