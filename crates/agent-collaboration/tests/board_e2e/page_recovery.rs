//! BR04/05: unavailable aggregate members stay explicit and retain their unread resume positions.
use super::fixture::{BoardFixture, at, fields, header, human, identity};
use codex_router_test_support::FixtureResult;
use serde_json::{Value, json};

pub async fn main_post(
    fixture: &BoardFixture,
    project: &str,
    revision: u64,
) -> FixtureResult<String> {
    let author = human(&format!("main-author-{}", identity()));
    fixture
        .accept(
            "role_grant",
            json!({"requestId":identity(),"protocolVersion":1,
        "actingFor":null,"actor":fixture.owner,"projectId":project,"operation":{"kind":"roleGrant",
        "subject":{"kind":"project","subject":project},"role":"advisor","holder":author,
        "model":null,"lineage":null,"expectedRevision":revision,"ownerGrant":null}}),
        )
        .await?;
    fixture.thread(project, &author).await
}

fn post_ids(reply: &Value) -> FixtureResult<Vec<Value>> {
    at(reply, "/page/records")?
        .as_array()
        .ok_or("post page")?
        .iter()
        .map(|row| at(row, "/postId").cloned())
        .collect()
}

pub async fn unavailable_sibling(fixture: &mut BoardFixture) -> FixtureResult {
    // Wrong build: a bad nonselected watched project drops healthy rows or disappears from unavailable.
    fixture.create().await?;
    let reader = human("unavailable-reader");
    fixture.initialize(&reader, &fixture.project).await?;
    let healthy = main_post(fixture, &fixture.project, 1).await?;
    let (_, broken) = fixture.foreign_project().await?;
    let thread = fixture.thread(&broken, &fixture.owner).await?;
    fixture
        .accept(
            "thread_watch",
            fields(
                header(&reader),
                json!({
        "thread":{"project":broken,"thread":thread}}),
            )?,
        )
        .await?;
    fixture.stop_host().await?;
    let original = super::stopped_faults::newer_schema(fixture, &broken).await?;
    fixture.reopen_host().await?;
    let first = fixture.inbox(&reader, json!({})).await?;
    assert_eq!(super::scope_inbox::post_ids(&first)?, vec![json!(healthy)]);
    assert_eq!(first.pointer("/page/unavailable"), Some(&json!([broken])));
    fixture.stop_host().await?;
    super::stopped_faults::restore_schema(fixture, &broken, original).await?;
    super::stopped_faults::corrupt_watch(fixture, &broken, &thread, &reader).await?;
    fixture.reopen_host().await?;
    let second = fixture.inbox(&reader, json!({})).await?;
    assert_eq!(super::scope_inbox::post_ids(&second)?, vec![json!(healthy)]);
    assert_eq!(second.pointer("/page/unavailable"), Some(&json!([broken])));
    let direct = fixture.host()?.mcp_reply("thread_show",json!({
        "thread":{"project":broken,"thread":thread},"reader":{"kind":"identified","reader":reader}})).await?;
    assert!(
        direct.is_error,
        "corrupt selected watch silently appeared readable: {}",
        direct.value
    );
    assert_eq!(direct.value.get("kind"), Some(&json!("invalidRecord")));
    assert_eq!(direct.value.get("stage"), Some(&json!("inspection")));
    assert_eq!(
        direct.value.get("nextAction"),
        Some(&json!("inspectResource"))
    );
    Ok(())
}

pub async fn aggregate_cursor_recovery(fixture: &mut BoardFixture) -> FixtureResult {
    // Wrong build: retry advances unavailable B, skips its behind-sweep post, or repeats retained A.
    fixture.create().await?;
    let broken = identity();
    fixture
        .create_project(&fixture.board, &broken, "returning cursor member")
        .await?;
    let behind = main_post(fixture, &broken, 1).await?;
    let first_healthy = main_post(fixture, &fixture.project, 1).await?;
    let second_healthy = main_post(fixture, &fixture.project, 2).await?;
    let saved = json!({"perProject":{
        (fixture.project.clone()):{"epoch":1,"position":0},
        (broken.clone()):{"epoch":1,"position":0}},"sweep":null});
    let selection = json!({"kind":"afterPosition","after":{"kind":"aggregate","cursor":saved}});
    let scope = json!({"kind":"board","boardId":fixture.board});
    fixture.stop_host().await?;
    let original = super::stopped_faults::newer_schema(fixture, &broken).await?;
    fixture.reopen_host().await?;
    let first = fixture
        .host()?
        .mcp(
            "post_list",
            json!({"scope":scope,"selection":selection,
        "page":{"limit":1}}),
        )
        .await?;
    assert_eq!(post_ids(&first)?, vec![json!(first_healthy)]);
    assert_eq!(first.pointer("/page/unavailable"), Some(&json!([broken])));
    let positions = at(&first, "/page/next/perProject")?
        .as_object()
        .ok_or("aggregate positions")?;
    assert_eq!(
        positions.get(&broken),
        Some(&json!({"epoch":1,"position":0}))
    );
    let mut cursor = at(&first, "/page/nextCursor")?.clone();
    assert!(cursor.is_string(), "healthy continuation must be present");
    fixture.stop_host().await?;
    super::stopped_faults::restore_schema(fixture, &broken, original).await?;
    fixture.reopen_host().await?;
    let mut seen = vec![json!(first_healthy)];
    for _ in 0..3 {
        let page = fixture
            .host()?
            .mcp(
                "post_list",
                json!({"scope":scope,"selection":selection,
            "page":{"limit":1,"cursor":cursor}}),
            )
            .await?;
        seen.extend(post_ids(&page)?);
        cursor = at(&page, "/page/nextCursor")?.clone();
        if cursor.is_null() {
            break;
        }
    }
    assert_eq!(
        seen,
        vec![json!(first_healthy), json!(behind), json!(second_healthy)]
    );
    assert!(cursor.is_null(), "aggregate continuation did not terminate");
    fixture.stop_host().await?;
    super::stopped_faults::newer_schema(fixture, &broken).await?;
    super::stopped_faults::newer_schema(fixture, &fixture.project).await?;
    fixture.reopen_host().await?;
    let unavailable = fixture
        .host()?
        .mcp(
            "post_list",
            json!({"scope":scope,"selection":selection,
        "page":{"limit":1}}),
        )
        .await?;
    assert_eq!(post_ids(&unavailable)?, Vec::<Value>::new());
    assert_eq!(unavailable.pointer("/page/nextCursor"), Some(&Value::Null));
    assert_eq!(unavailable.pointer("/page/next"), Some(&saved));
    let actual = at(&unavailable, "/page/unavailable")?
        .as_array()
        .ok_or("unavailable members")?
        .iter()
        .map(|value| value.as_str().ok_or("unavailable project ID"))
        .collect::<Result<std::collections::BTreeSet<_>, _>>()?;
    assert_eq!(
        actual,
        [fixture.project.as_str(), broken.as_str()]
            .into_iter()
            .collect()
    );
    Ok(())
}
