#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
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
#[path = "support/board_read_setup.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
mod board_read_setup;
use board_fixture::{BoardFixture, TestError, accepted};
use board_read_setup::*;
use message_board::*;
use serde_json::json;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove real reader-state effects; Result propagates setup errors."
)]
async fn an_archived_content_read_advances_its_mark_without_activity_or_a_request_key()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let client = fixture.api.client("archived read mark proof").await?;
    let (_, board, project) = create_location(&fixture, "archived reader").await?;
    let root = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: project.clone(),
        },
        json!({"text":"archived thread"}),
    )
    .await?;
    let mut archive = fixture.header();
    archive["boardId"] = json!(board);
    accepted(&fixture.api.call("board_archive", archive).await?);
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("store")?;
    let mut snapshot = store.snapshot().await?;
    let head = snapshot.high_water();
    let requests = snapshot.records(RecordKind::Request).await?.len();
    snapshot.finish().await?;
    let reader = human_reader("archived-reader")?;
    let thread = ThreadId::from(root);
    let result = client
        .thread_show(ThreadShowRequest {
            include_redacted: false,
            thread: (thread.clone()).into(),
            reader: ReadActor::Identified {
                reader: reader.clone(),
            },
        })
        .await?;
    assert_eq!(result.read_mark, Some(ReadMarkOutcome::Unchanged {}));
    let content = client
        .feed_read(FeedReadRequest {
            project_id: project.clone(),
            reader: ReadActor::Identified {
                reader: reader.clone(),
            },
            after: FeedCursor {
                epoch: head.epoch,
                position: FeedPosition::ZERO,
            },
            limit: PageLimit::try_from(100)?,
            include_redacted: true,
        })
        .await?;
    assert_eq!(
        content.read_mark,
        Some(ReadMarkOutcome::Advanced { through: head })
    );
    assert!(
        result
            .watch_status
            .ok_or("watch")?
            .message
            .contains("read-only")
    );
    let mut snapshot = store.snapshot().await?;
    let mark = snapshot
        .read_mark(
            &reader,
            &ReadScope::Project {
                project_id: project.clone(),
            },
        )
        .await?
        .ok_or("mark")?;
    assert_eq!(mark.through, head);
    assert_eq!(
        mark.envelope.actor.declared_identity().as_ref(),
        Some(&reader)
    );
    assert!(mark.envelope.request.is_none());
    assert_eq!(snapshot.high_water(), head);
    assert_eq!(snapshot.records(RecordKind::Request).await?.len(), requests);
    snapshot.finish().await?;
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The data must still answer when stand-down refuses its reader-state effect."
)]
async fn an_archived_read_answers_data_but_cannot_bypass_unacknowledged_stand_down()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let root = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: project.clone(),
        },
        json!({"actor":{"kind":"human","humanId":"root-author"},"text":"still readable"}),
    )
    .await?;
    let new_holder = json!({"kind":"human","humanId":"new-lead"});
    let owner_grant = OwnerGrantId::generate();
    accepted(&fixture.api.call("owner_grant_record",fixture.request(&project,json!({"kind":"ownerGrantRecord","grantId":owner_grant,"grant":{"kind":"replaceLead","project":project,"newHolder":new_holder,"reason":"owner relayed replacement"}}))).await?);
    accepted(&fixture.api.call("role_replace_lead",fixture.request(&project,json!({"kind":"roleReplaceLead","newHolder":new_holder,"ownerGrant":owner_grant,"reason":"replace","expectedAssignmentRevision":1}))).await?);
    let mut archive = fixture.header();
    archive["boardId"] = json!(fixture.board);
    accepted(&fixture.api.call("board_archive", archive).await?);
    let result=fixture.api.call("thread_show",json!({"thread":{"thread":ThreadId::from(root.clone())},"reader":{"kind":"identified","reader":fixture.actor}})).await?;
    assert_eq!(
        result.pointer("/result/threadId"),
        Some(&json!(ThreadId::from(root.clone()))),
        "{result}"
    );
    assert_eq!(
        result.pointer("/result/readMark/kind"),
        Some(&json!("unchanged"))
    );
    let content = fixture.api.call("feed_read",json!({"projectId":project,"reader":{"kind":"identified","reader":fixture.actor},"after":{"epoch":1,"position":0},"limit":100,"includeRedacted":true})).await?;
    assert_eq!(
        content.pointer("/result/readMark/kind"),
        Some(&json!("notAdvanced")),
        "{result}"
    );
    assert_eq!(
        content.pointer("/result/readMark/reason/kind"),
        Some(&json!("standDownUnacknowledged"))
    );
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("store")?;
    let mut snapshot = store.snapshot().await?;
    let reader: Identity = serde_json::from_value(fixture.actor.clone())?;
    assert!(
        snapshot
            .read_mark(
                &reader,
                &ReadScope::Thread {
                    thread_id: ThreadId::from(root)
                }
            )
            .await?
            .is_none()
    );
    snapshot.finish().await?;
    fixture.stop().await
}
