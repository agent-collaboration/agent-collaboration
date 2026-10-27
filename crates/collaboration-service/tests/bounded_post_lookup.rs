//! Cold bare-post lookup must not open any of the 8/80 unrelated native files.
#[path = "support/board_fixture.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
pub mod board_fixture;
#[path = "support/board_read_setup.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
mod board_read_setup;
#[path = "support/read_cost_counter.rs"]
#[expect(
    dead_code,
    reason = "Shared test fixture helpers serve different callers."
)]
mod read_cost_counter;
use board_fixture::{BoardFixture, TestError};
use board_read_setup::post_at_placement;
use message_board::*;
use read_cost_counter::{ReadCost, current_cost};
use serde_json::json;
use sqlx::{ConnectOptions, Connection};

async fn selected_bodies_and_duplicate_ids_remain_strict() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let client = fixture.api.client("post lookup validation").await?;
    let (first, _) = fixture.project().await?;
    let (second, _) = fixture.project().await?;
    let duplicate = PostId::generate();
    for project in [&first, &second] {
        board_fixture::accepted(&board_read_setup::keyed_call(&fixture,"thread_create",json!({"projectId":project,"postId":duplicate,"text":"duplicated post ID","about":{"kind":"project","subject":{"line":"Fixture discussion"}},"references":[],"addressees":[],"role":null,"watch":false})).await?);
    }
    let error = client
        .post_show(PostShowRequest { post_id: duplicate })
        .await
        .err()
        .ok_or("ambiguous post was accepted")?;
    assert!(
        matches!(error,collaboration_client::BoardClientError::Rejected(error) if error.kind==BoardFailureKind::InvalidField && error.message.contains("ambiguous"))
    );
    let corrupt=post_at_placement(&fixture,Placement::Project {project_id:first.clone()},json!({"actor":{"kind":"human","humanId":"corrupt-fixture author"},"text":"selected corrupt body"})).await?;
    let healthy=post_at_placement(&fixture,Placement::Project {project_id:second.clone()},json!({"actor":{"kind":"human","humanId":"healthy-fixture author"},"text":"healthy beside corruption"})).await?;
    let store = fixture
        ._registry
        .existing_project(&first)
        .await?
        .ok_or("first project")?;
    let mut raw = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .connect()
        .await?;
    let changed = sqlx::query("UPDATE posts SET text='' WHERE post_id=?")
        .bind(corrupt.as_str())
        .execute(&mut raw)
        .await?;
    assert_eq!(changed.rows_affected(), 1);
    assert!(
        client
            .post_show(PostShowRequest { post_id: corrupt })
            .await
            .is_err()
    );
    let shown = client
        .post_show(PostShowRequest {
            post_id: healthy.clone(),
        })
        .await?
        .0;
    assert_eq!(shown.post_id, healthy);
    assert_eq!(shown.project_id, second);
    assert_eq!(shown.text.as_str(), "healthy beside corruption");
    raw.close().await?;
    fixture.stop().await
}

async fn lookup_costs(size: usize) -> Result<(ReadCost, ReadCost), TestError> {
    let fixture = BoardFixture::start().await?;
    let client = fixture.api.client("cold post lookup").await?;
    let (project, _) = fixture.project().await?;
    let post = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: project.clone(),
        },
        json!({"text":"the selected post"}),
    )
    .await?;
    let known = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("open target")?;
    let mut read = known.snapshot().await?;
    let Some(Record::Project(template)) = read.record(&project.clone().into()).await? else {
        return Err("project template".into());
    };
    read.finish().await?;
    // These are initialized physical stores, closed before observation, never
    // opened through the observed Router registry. Only the target handle is open.
    for index in 1..=size {
        let sibling = ProjectId::try_from(format!("00000000-0000-7000-8000-{index:012x}"))?;
        assert!(sibling < project);
        let seed = board_fixture::native_registry::new_registry(fixture._directory.path())?;
        let input: ProjectCreateRequest = serde_json::from_value(json!({
            "requestId":RequestId::generate(),"protocolVersion":1,"actor":fixture.actor,
            "projectId":sibling,"boardId":fixture.board,"title":sibling.as_str()
        }))?;
        board_fixture::registered_project::prepare_registered_creation(
            &fixture._fleet,
            &seed,
            &input,
            &fixture.record_origin()?,
            &runtime_primitives::SystemClock,
        )
        .await?;
        let store = seed.open_project(&sibling).await?;
        let mut state = template.clone();
        state.project_id = sibling.clone();
        state.envelope.id = sibling.clone().into();
        state.envelope.scope.project = Some(sibling);
        state.envelope.request = None;
        let mut write = store.begin_admission(fixture.record_origin()?).await?;
        write.append_record(Record::Project(state)).await?;
        write.commit().await?;
        drop(store);
        std::sync::Arc::try_unwrap(seed)
            .map_err(|_| "seed registry still owned")?
            .close()
            .await?;
    }
    let before = current_cost();
    let shown = client
        .post_show(PostShowRequest {
            post_id: post.clone(),
        })
        .await?
        .0;
    let found = current_cost().since(before);
    assert_eq!(shown.post_id, post);
    assert_eq!(shown.project_id, project);
    assert_eq!(shown.text.as_str(), "the selected post");
    let before = current_cost();
    let missing = client
        .post_show(PostShowRequest {
            post_id: PostId::generate(),
        })
        .await
        .err()
        .ok_or("missing post was accepted")?;
    let absent = current_cost().since(before);
    assert!(
        matches!(missing,collaboration_client::BoardClientError::Rejected(error) if error.kind==BoardFailureKind::ResourceNotFound)
    );
    fixture.stop().await?;
    Ok((found, absent))
}
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn post_lookup_costs_ignore_unopened_project_files() -> Result<(), TestError> {
    read_cost_counter::install();
    let small = Box::pin(lookup_costs(8)).await?;
    let large = Box::pin(lookup_costs(80)).await?;
    eprintln!("cold post lookup: small={small:?}; large={large:?}");
    assert_eq!(small, large, "post lookup grew with unopened files");
    assert_eq!(large.0.project_opens, 0);
    assert_eq!(large.1.project_opens, 0);
    assert_eq!(large.0.records, 1);
    assert_eq!(large.1.records, 0);
    assert!(large.0.statements > 0 && large.1.statements > 0);
    selected_bodies_and_duplicate_ids_remain_strict().await?;
    Ok(())
}
