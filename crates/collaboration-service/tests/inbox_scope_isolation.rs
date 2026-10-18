//! Watched context follows the reader; unreadable siblings remain explicit metadata.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
#[path = "support/board_read_setup.rs"]
mod board_read_setup;
use board_fixture::{BoardFixture, TestError, accepted};
use board_read_setup::*;
use message_board::*;
use serde_json::json;
use sqlx::Connection;

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn a_project_watch_covers_preexisting_threads_without_materialized_thread_watches()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let client = fixture.api.client("project watch context").await?;
    let (own, _) = fixture.project().await?;
    let (foreign_workspace, foreign_board, foreign) =
        create_location(&fixture, "foreign project watch").await?;
    let reader = human_reader("project-watch reader")?;
    client
        .inbox_fetch(initialized_request(own.clone(), reader.clone())?)
        .await?;
    let own_post = post_at_placement(
        &fixture,
        Placement::Project { project_id: own },
        json!({"text":"own unread"}),
    )
    .await?;
    let root = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: foreign.clone(),
        },
        json!({"text":"root before project watch"}),
    )
    .await?;
    let thread = ThreadId::from(root.clone());
    let old_reply = post_at_placement(
        &fixture,
        Placement::Thread {
            thread_id: thread.clone(),
        },
        json!({"text":"reply before project watch"}),
    )
    .await?;
    accepted(
        &keyed_call(
            &fixture,
            "project_watch",
            json!({"projectId":foreign,"actor":reader,"observedEpoch":1}),
        )
        .await?,
    );
    let store = fixture
        ._registry
        .existing_project(&foreign)
        .await?
        .ok_or("foreign store")?;
    let mut read = store.begin_admission().await?;
    assert!(
        !read.thread_watch_exists(&thread, &reader).await?,
        "project coverage must not depend on a thread-watch row"
    );
    read.rollback().await?;
    accepted(
        &keyed_call(
            &fixture,
            "thread_resolve",
            json!({"thread":{"project":foreign,"thread":thread},"reason":"finished"}),
        )
        .await?,
    );
    let scope = InboxScope::Workspace {
        workspace_id: fixture.workspace.clone(),
    };
    let unread = client
        .inbox_fetch(inbox_request(
            scope.clone(),
            reader.clone(),
            InboxReadMode::Unread,
        )?)
        .await?
        .page;
    assert!(unread.unavailable.is_empty());
    assert_eq!(inbox_post_ids(&unread.records), vec![own_post.clone()]);
    assert_eq!(unread.records.len(), 2);
    let states: Vec<_> = unread
        .records
        .iter()
        .filter_map(|record| match record {
            InboxActivity::ThreadStateChanged {
                workspace_id,
                board_id,
                thread_id,
                project_id,
                state,
                ..
            } => Some((workspace_id, board_id, thread_id, project_id, state)),
            _ => None,
        })
        .collect();
    assert_eq!(
        states,
        vec![(
            &foreign_workspace,
            &foreign_board,
            &thread,
            &foreign,
            &ThreadState::Resolved
        )]
    );
    assert!(unread.next_cursor.is_none());
    let latest = client
        .inbox_fetch(inbox_request(scope, reader.clone(), InboxReadMode::Latest)?)
        .await?
        .page;
    assert_eq!(latest.records.len(), 2);
    assert_eq!(
        inbox_post_ids(&latest.records)
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>(),
        [own_post, old_reply].into_iter().collect()
    );
    assert!(!inbox_post_ids(&latest.records).contains(&root));
    assert!(latest.next_cursor.is_none());
    let mut read = store.begin_admission().await?;
    assert!(!read.thread_watch_exists(&thread, &reader).await?);
    read.rollback().await?;
    fixture.stop().await
}

#[derive(Clone, Copy)]
enum SiblingFault {
    TooNew,
    WatchBoolean,
    WatchReal,
}

#[allow(clippy::panic_in_result_fn)]
async fn healthy_inbox_reports_sibling(fault: SiblingFault) -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let client = fixture.api.client("sibling failure inbox").await?;
    let (own, _) = fixture.project().await?;
    let (_, _, foreign) = create_location(&fixture, "unavailable nonselected").await?;
    let reader = human_reader("isolation reader")?;
    client
        .inbox_fetch(initialized_request(own.clone(), reader.clone())?)
        .await?;
    let healthy = post_at_placement(
        &fixture,
        Placement::Project { project_id: own },
        json!({"text":"healthy activity"}),
    )
    .await?;
    let root = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: foreign.clone(),
        },
        json!({"text":"foreign root"}),
    )
    .await?;
    let thread = ThreadId::from(root);
    accepted(
        &keyed_call(
            &fixture,
            "thread_watch",
            json!({"thread":{"project":foreign,"thread":thread},"actor":reader}),
        )
        .await?,
    );
    drop(client);
    let store = fixture
        ._registry
        .existing_project(&foreign)
        .await?
        .ok_or("foreign store")?;
    store.close_connections().await;
    let mut raw = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new().filename(store.path()),
    )
    .await?;
    let changed = match fault {
        SiblingFault::TooNew => {
            let supported = message_board_storage::PROJECT_MIGRATOR
                .iter()
                .map(|migration| migration.version)
                .max()
                .ok_or("project migration")?;
            sqlx::query("UPDATE _sqlx_migrations SET version=? WHERE version=?")
                .bind(supported + 1)
                .bind(supported)
                .execute(&mut raw)
                .await?
        }
        SiblingFault::WatchBoolean | SiblingFault::WatchReal => {
            sqlx::query("PRAGMA ignore_check_constraints=ON")
                .execute(&mut raw)
                .await?;
            let update = match fault {
                SiblingFault::WatchReal => {
                    "UPDATE thread_watches SET active=1.5 WHERE reader=? AND thread_id=?"
                }
                _ => "UPDATE thread_watches SET active=2 WHERE reader=? AND thread_id=?",
            };
            sqlx::query(update)
                .bind(serde_json::to_string(&reader)?)
                .bind(thread.as_str())
                .execute(&mut raw)
                .await?
        }
    };
    assert_eq!(changed.rows_affected(), 1);
    raw.close().await?;
    drop(store);
    let fixture = fixture.restart_read_fixture().await?;
    let client = fixture.api.client("reopened sibling failure inbox").await?;
    let page = client
        .inbox_fetch(inbox_request(
            InboxScope::Workspace {
                workspace_id: fixture.workspace.clone(),
            },
            reader,
            InboxReadMode::Unread,
        )?)
        .await?
        .page;
    assert_eq!(page.unavailable, vec![foreign]);
    assert_eq!(inbox_post_ids(&page.records), vec![healthy]);
    assert_eq!(page.records.len(), 1);
    assert!(page.next_cursor.is_none());
    fixture.stop().await
}
#[tokio::test]
async fn a_nonselected_too_new_store_is_reported_beside_the_healthy_page() -> Result<(), TestError>
{
    healthy_inbox_reports_sibling(SiblingFault::TooNew).await
}
#[tokio::test]
async fn a_nonselected_corrupt_watch_is_reported_beside_the_healthy_page() -> Result<(), TestError>
{
    healthy_inbox_reports_sibling(SiblingFault::WatchBoolean).await
}

#[tokio::test]
async fn a_nonselected_real_watch_flag_is_refused_without_integer_coercion() -> Result<(), TestError>
{
    healthy_inbox_reports_sibling(SiblingFault::WatchReal).await
}
