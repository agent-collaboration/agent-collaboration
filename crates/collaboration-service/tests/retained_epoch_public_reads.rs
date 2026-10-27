//! Public readers preserve assigning epochs across a real native snapshot install.
#[path = "support/accepted_snapshot.rs"]
pub mod accepted_snapshot;
#[path = "retained_epoch_public_reads/activity_reads.rs"]
mod activity_reads;
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
#[path = "support/timed_board_posts.rs"]
mod timed_board_posts;
use board_fixture::{BoardFixture, TestError};
use message_board::*;
use std::collections::BTreeMap;
use timed_board_posts::{TimedPostInput, append_post_at};

async fn post_cursor(
    fixture: &BoardFixture,
    project: &ProjectId,
    post: &PostId,
) -> Result<FeedCursor, TestError> {
    let store = fixture
        ._registry
        .existing_project(project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    let record = read.record(&post.clone().into()).await?.ok_or("post")?;
    read.finish().await?;
    record
        .envelope()
        .position
        .project_cursor()
        .ok_or_else(|| "post cursor".into())
}

async fn append_project_revision(
    fixture: &BoardFixture,
    project: &ProjectId,
) -> Result<FeedCursor, TestError> {
    let store = fixture
        ._registry
        .existing_project(project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    let Some(Record::Project(mut state)) = read.record(&project.clone().into()).await? else {
        return Err("project state".into());
    };
    read.finish().await?;
    state.revision = state.revision.successor()?;
    state.envelope.request = None;
    let mut write = store.begin_admission(fixture.record_origin()?).await?;
    let cursor = write.append_record(Record::Project(state)).await?;
    write.commit().await?;
    Ok(cursor)
}

fn aggregate(project: &ProjectId, cursor: FeedCursor) -> ReadPosition {
    ReadPosition::Aggregate {
        cursor: AggregateCursor {
            per_project: BTreeMap::from([(project.clone(), cursor)]),
            sweep: None,
        },
    }
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn range_predecessor_and_public_next_reuse_preserve_actual_assigning_epochs()
-> Result<(), TestError> {
    let source = BoardFixture::start().await?;
    let (project, _) = source.project().await?;
    let old = append_post_at(
        &source,
        &project,
        TimedPostInput {
            time: "2026-10-12T00:01:00Z",
            thread: None,
        },
    )
    .await?;
    let old_cursor = post_cursor(&source, &project, &old).await?;
    let mut target = BoardFixture::start().await?;
    target.workspace = source.workspace.clone();
    target.board = source.board.clone();
    let (inherited, epoch) =
        accepted_snapshot::bootstrap_project(&source, &target, &project).await?;
    assert_eq!(inherited, old_cursor);
    let first_new = append_project_revision(&target, &project).await?;
    assert_eq!(first_new.epoch, epoch);
    assert_eq!(first_new.position.get(), inherited.position.get() + 1);
    let client = target.api.client("retained epoch reads").await?;
    let scope = PostListScope::Project {
        project_id: project.clone(),
    };
    // This range has no posts. Its public next must still be an actual row,
    // not the fabricated (new epoch, inherited position).
    let page = client
        .post_list(PostListRequest {
            scope: scope.clone(),
            selection: PostSelection::Range {
                from: aggregate(&project, first_new),
                to: aggregate(&project, first_new),
            },
            page: PageRequest::default(),
        })
        .await?
        .page;
    assert!(page.records.is_empty());
    assert_eq!(page.next.per_project.get(&project), Some(&inherited));
    assert!(page.unavailable.is_empty());
    let later = append_post_at(
        &target,
        &project,
        TimedPostInput {
            time: "2026-10-12T00:02:00Z",
            thread: None,
        },
    )
    .await?;
    let later_cursor = post_cursor(&target, &project, &later).await?;
    let next = client
        .post_list(PostListRequest {
            scope: scope.clone(),
            selection: PostSelection::AfterPosition {
                after: ReadPosition::Aggregate { cursor: page.next },
            },
            page: PageRequest::default(),
        })
        .await?
        .page;
    assert_eq!(
        next.records
            .iter()
            .map(|post| post.post_id.clone())
            .collect::<Vec<_>>(),
        vec![later.clone()]
    );
    // Selection decoding must allow mixed epochs; the native snapshot, not
    // their tag equality, decides whether these actual coordinates survive.
    let ranged = client
        .post_list(PostListRequest {
            scope: scope.clone(),
            selection: PostSelection::Range {
                from: ReadPosition::Project { cursor: old_cursor },
                to: ReadPosition::Project {
                    cursor: later_cursor,
                },
            },
            page: PageRequest::default(),
        })
        .await?
        .page;
    assert_eq!(
        ranged
            .records
            .iter()
            .map(|post| post.post_id.clone())
            .collect::<Vec<_>>(),
        vec![old.clone(), later]
    );
    drop(client);
    let target = target.restart_read_fixture().await?;
    let client = target.api.client("retained after reopen").await?;
    let mut latest = PostListRequest {
        scope,
        selection: PostSelection::Latest,
        page: PageRequest {
            limit: PageLimit::try_from(1)?,
            cursor: None,
        },
    };
    let first = client.post_list(latest.clone()).await?.page;
    latest.page.cursor = Some(first.next_cursor.ok_or("retained continuation")?);
    let second = client.post_list(latest).await?.page;
    assert_eq!(second.records.first().map(|post| &post.post_id), Some(&old));
    assert_eq!(second.next.per_project.get(&project), Some(&old_cursor));
    drop(client);
    target.stop().await?;
    source.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn a_discarded_old_suffix_is_refused_even_when_new_epoch_reuses_its_position()
-> Result<(), TestError> {
    let source = BoardFixture::start().await?;
    let (project, _) = source.project().await?;
    append_post_at(
        &source,
        &project,
        TimedPostInput {
            time: "2026-10-12T00:01:00Z",
            thread: None,
        },
    )
    .await?;
    let mut target = BoardFixture::start().await?;
    target.workspace = source.workspace.clone();
    target.board = source.board.clone();
    accepted_snapshot::bootstrap_project(&source, &target, &project).await?;
    let discarded = append_post_at(
        &source,
        &project,
        TimedPostInput {
            time: "2026-10-12T00:02:00Z",
            thread: None,
        },
    )
    .await?;
    let discarded = post_cursor(&source, &project, &discarded).await?;
    let replacement = append_post_at(
        &target,
        &project,
        TimedPostInput {
            time: "2026-10-12T00:03:00Z",
            thread: None,
        },
    )
    .await?;
    let replacement = post_cursor(&target, &project, &replacement).await?;
    assert_eq!(replacement.position, discarded.position);
    assert_ne!(replacement.epoch, discarded.epoch);
    let client = target.api.client("invalid retained epochs").await?;
    for cursor in [
        discarded,
        FeedCursor {
            epoch: EpochNumber::try_from(99)?,
            ..replacement
        },
        FeedCursor {
            position: replacement.position.successor()?,
            ..replacement
        },
    ] {
        let result = client
            .post_list(PostListRequest {
                scope: PostListScope::Project {
                    project_id: project.clone(),
                },
                selection: PostSelection::AfterPosition {
                    after: ReadPosition::Project { cursor },
                },
                page: PageRequest::default(),
            })
            .await;
        assert!(
            result.is_err(),
            "invalid coordinate was silently retagged: {cursor:?}"
        );
    }
    drop(client);
    target.stop().await?;
    source.stop().await
}
