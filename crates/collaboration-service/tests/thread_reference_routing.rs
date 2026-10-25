//! Thread references route directly or probe only initialized local store handles.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
#[path = "support/board_read_setup.rs"]
mod board_read_setup;
#[path = "support/read_cost_counter.rs"]
mod read_cost_counter;
use board_fixture::{BoardFixture, TestError, accepted};
use board_read_setup::{create_registered_project, post_at_placement};
use message_board::*;
use read_cost_counter::{ReadCost, current_cost, project_statements};
use serde_json::json;
use sqlx::{ConnectOptions, Connection};

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn qualified_routes_and_bare_probes_never_open_another_store() -> Result<(), TestError> {
    read_cost_counter::install();
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let other = ProjectId::generate();
    accepted(
        &create_registered_project(
            &fixture,
            json!({"boardId":fixture.board,
        "projectId":other,"title":"unopened sibling"}),
        )
        .await?,
    );
    let root = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: project.clone(),
        },
        json!({"text":"target"}),
    )
    .await?;
    let unrelated = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: project.clone(),
        },
        json!({"actor":{"kind":"human","humanId":"other author"},"text":"unrelated"}),
    )
    .await?;
    let foreign = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: other.clone(),
        },
        json!({"text":"sibling thread"}),
    )
    .await?;
    let fixture = fixture.restart_read_fixture().await?;
    let client = fixture.api.client("thread ref routing").await?;
    let bare = ThreadRef::from(ThreadId::from(root));
    let foreign = ThreadRef::from(ThreadId::from(foreign));
    let request = |thread| ThreadShowRequest {
        include_redacted: false,
        thread,
        reader: ReadActor::Anonymous {},
    };
    let before = current_cost();
    let error = client
        .thread_show(request(bare.clone()))
        .await
        .expect_err("no open projects");
    assert!(
        error
            .to_string()
            .contains("Thread not found in open local projects"),
        "{error}"
    );
    assert_eq!(
        current_cost().since(before),
        ReadCost::default(),
        "bare lookup opened a store"
    );
    let sibling_before = project_statements(other.as_str());
    let qualified = ThreadRef {
        project: Some(project.clone()),
        ..bare.clone()
    };
    let shown = client.thread_show(request(qualified.clone())).await?;
    assert_eq!(shown.project_id, project);
    assert_eq!(shown.thread.thread_id, bare.thread);
    assert_eq!(project_statements(other.as_str()), sibling_before);
    // Warm both routes; a bare lookup adds exactly its one metadata PK probe.
    let before = current_cost();
    client.thread_show(request(qualified.clone())).await?;
    let direct_cost = current_cost().since(before);
    let before = current_cost();
    client.thread_show(request(bare.clone())).await?;
    let probe_cost = current_cost().since(before);
    assert_eq!(probe_cost.statements, direct_cost.statements + 1);
    assert_eq!(probe_cost.records, direct_cost.records);
    assert_eq!(probe_cost.feed_entries, direct_cost.feed_entries);
    let before = current_cost();
    client
        .thread_show(request(foreign.clone()))
        .await
        .expect_err("sibling is still unopened");
    let missing_cost = current_cost().since(before);
    assert_eq!(
        missing_cost.statements, 1,
        "only the open project gets a PK probe"
    );
    assert_eq!(missing_cost.records, 0);
    assert_eq!(project_statements(other.as_str()), sibling_before);
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut observer = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .connect()
        .await?;
    let changed = sqlx::query("UPDATE threads SET state='unsupportedState' WHERE thread_id=?")
        .bind(unrelated.as_str())
        .execute(&mut observer)
        .await?;
    assert_eq!(changed.rows_affected(), 1);
    client.thread_show(request(bare.clone())).await?;
    assert!(
        client
            .thread_show(request(ThreadRef {
                thread: ThreadId::from(unrelated),
                project: Some(project.clone())
            }))
            .await
            .is_err()
    );
    observer.close().await?;
    let foreign_qualified = ThreadRef {
        project: Some(other.clone()),
        ..foreign.clone()
    };
    assert_eq!(
        client
            .thread_show(request(foreign_qualified.clone()))
            .await?
            .project_id,
        other
    );
    assert_eq!(
        client
            .thread_show(request(foreign.clone()))
            .await?
            .project_id,
        other
    );
    assert!(
        client
            .thread_show(request(ThreadRef {
                project: Some(other.clone()),
                ..bare.clone()
            }))
            .await
            .is_err(),
        "qualified lookup escaped its project"
    );
    // Both stores are now open; the first hit ends the bare search before its sibling.
    let sibling_before = project_statements(other.as_str());
    client.thread_show(request(bare)).await?;
    assert_eq!(project_statements(other.as_str()), sibling_before);
    eprintln!(
        "thread ref costs: direct={direct_cost:?}, bare={probe_cost:?}, missing={missing_cost:?}"
    );
    fixture.stop().await
}
