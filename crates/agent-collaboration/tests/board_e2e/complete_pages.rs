//! BR09 / VD §5a U2,U26–31,U42,U47: continuations cross CLI/MCP without loss or duplication.
use super::fixture::machine_progress;
use super::fixture::{BoardFixture, accepted, at, fields, header, human};
use codex_router_test_support::FixtureResult;
use serde_json::{Value, json};

fn records(page: &Value, field: &str) -> FixtureResult<Vec<Value>> {
    at(page, "/page/records")?
        .as_array()
        .ok_or("page records")?
        .iter()
        .map(|row| at(row, field).cloned())
        .collect()
}

struct PagingCase {
    thread: String,
    second: String,
    second_thread: String,
    posts: Vec<Value>,
    reader: Value,
}

pub async fn complete_pages(fixture: &mut BoardFixture) -> FixtureResult {
    // Keep independent complete-set expectations through each real public paging path.
    fixture.create().await?;
    let case = prepare_pages(fixture).await?;
    traverse_post_scopes(fixture, &case).await?;
    traverse_participants(fixture, &case).await?;
    traverse_threads(fixture, &case).await?;
    watch_and_unwatch(fixture, &case).await?;
    isolate_corrupt_sibling(fixture, &case).await?;
    archive_workspace(fixture).await
}

async fn prepare_pages(fixture: &BoardFixture) -> FixtureResult<PagingCase> {
    let public_lifecycle=fixture.write("fact_record",fixture.request(&fixture.owner,json!({"kind":"factRecord",
        "fact":{"factId":super::fixture::identity(),"kind":{"kind":"lifecycle","fact":{"kind":"archiveSeen","board":fixture.board}},
        "links":[],"producerTime":chrono::Utc::now(),"anchor":null}}))).await?;
    super::role_checks::rejected(
        &public_lifecycle,
        json!([{"kind":"invalidShape","reason":{"field":"kind"}}]),
    )?;
    let thread = fixture.thread(&fixture.project, &fixture.owner).await?;
    let second = super::fixture::identity();
    fixture
        .create_project(&fixture.board, &second, &format!("second {second}"))
        .await?;
    let second_thread = fixture.thread(&second, &fixture.owner).await?;
    let mut posts = Vec::new();
    for number in 0..3 {
        posts.push(
            fixture
                .reply(
                    &fixture.project,
                    &thread,
                    &format!("complete traversal {number}"),
                )
                .await?,
        );
    }
    Ok(PagingCase {
        thread,
        second,
        second_thread,
        posts,
        reader: human("page-reader"),
    })
}

async fn traverse_post_scopes(fixture: &BoardFixture, case: &PagingCase) -> FixtureResult {
    let PagingCase {
        thread,
        second_thread,
        posts,
        ..
    } = case;
    let mut expected = posts
        .iter()
        .rev()
        .map(|post| at(post, "/postId").cloned())
        .collect::<FixtureResult<Vec<_>>>()?;
    expected.push(json!(thread));
    let mut workspace_expected = expected.clone();
    workspace_expected.push(json!(second_thread));
    for (scope, flags, expected, ordered) in [
        (
            json!({"kind":"thread","thread":{"project":fixture.project,"thread":thread}}),
            vec![
                "--scope".to_owned(),
                "thread".to_owned(),
                "--thread-id".to_owned(),
                format!("{}/{thread}", fixture.project),
            ],
            expected,
            true,
        ),
        (
            json!({"kind":"workspace","workspaceId":fixture.workspace}),
            vec![
                "--scope".to_owned(),
                "workspace".to_owned(),
                "--workspace-id".to_owned(),
                fixture.workspace.clone(),
            ],
            workspace_expected,
            false,
        ),
    ] {
        traverse_post_pages(fixture, scope, flags, expected, ordered).await?;
    }
    Ok(())
}

async fn traverse_post_pages(
    fixture: &BoardFixture,
    scope: Value,
    flags: Vec<String>,
    mut expected: Vec<Value>,
    ordered: bool,
) -> FixtureResult {
    let mut observed = Vec::new();
    let mut cursor = Value::Null;
    for index in 0..expected.len() + 1 {
        let page = if index % 2 == 0 {
            fixture.host()?.mcp("post_list",json!({"scope":scope,"selection":{"kind":"latest"},"page":{"limit":1,"cursor":cursor}})).await?
        } else {
            let mut args = vec!["post".to_owned(), "list".to_owned()];
            args.extend(flags.clone());
            args.extend(["--limit".to_owned(), "1".to_owned()]);
            if let Some(cursor) = cursor.as_str() {
                args.extend(["--cursor".to_owned(), cursor.to_owned()]);
            }
            fixture
                .cli(&args.iter().map(String::as_str).collect::<Vec<_>>())
                .await?
        };
        observed.extend(records(&page, "/postId")?);
        cursor = at(&page, "/page/nextCursor")?.clone();
        if cursor.is_null() {
            break;
        }
    }
    if !ordered {
        expected.sort_by_key(Value::to_string);
        observed.sort_by_key(Value::to_string);
    }
    assert_eq!(observed, expected);
    assert!(cursor.is_null());
    Ok(())
}

async fn traverse_participants(fixture: &BoardFixture, case: &PagingCase) -> FixtureResult {
    let PagingCase { thread, reader, .. } = case;
    for actor in [&fixture.helper, reader] {
        fixture
            .accept(
                "thread_join",
                json!({"requestId":super::fixture::identity(),"protocolVersion":1,
            "actor":actor,"thread":{"project":fixture.project,"thread":thread},"role":"participant",
            "watch":false,"replace":null,"note":null}),
            )
            .await?;
    }
    let mut participants = Vec::new();
    let mut cursor = Value::Null;
    for _ in 0..3 {
        let page = fixture
            .host()?
            .mcp(
                "thread_participant_list",
                json!({"thread":{"project":fixture.project,"thread":thread},
            "page":{"limit":1,"cursor":cursor}}),
            )
            .await?;
        participants.extend(records(&page, "/identity")?);
        cursor = at(&page, "/page/nextCursor")?.clone();
        if cursor.is_null() {
            break;
        }
    }
    let mut expected = vec![fixture.helper.clone(), reader.clone()];
    expected.sort_by_key(Value::to_string);
    participants.sort_by_key(Value::to_string);
    assert_eq!(participants, expected);
    assert!(cursor.is_null());
    Ok(())
}

async fn traverse_threads(fixture: &BoardFixture, case: &PagingCase) -> FixtureResult {
    let PagingCase {
        thread,
        second_thread,
        reader,
        ..
    } = case;
    let mut threads = Vec::new();
    let mut cursor = Value::Null;
    for index in 0..3 {
        let page = if index % 2 == 0 {
            let mut args = vec![
                "thread".to_owned(),
                "list".to_owned(),
                "--workspace-id".to_owned(),
                fixture.workspace.clone(),
                "--reader".to_owned(),
                reader.to_string(),
                "--limit".to_owned(),
                "1".to_owned(),
            ];
            if let Some(cursor) = cursor.as_str() {
                args.extend(["--cursor".to_owned(), cursor.to_owned()]);
            }
            fixture
                .cli(&args.iter().map(String::as_str).collect::<Vec<_>>())
                .await?
        } else {
            fixture
                .host()?
                .mcp(
                    "thread_list",
                    json!({"workspaceId":fixture.workspace,"reader":reader,
            "watchedOnly":false,"page":{"limit":1,"cursor":cursor}}),
                )
                .await?
        };
        threads.extend(records(&page, "/threadId")?);
        cursor = at(&page, "/page/nextCursor")?.clone();
        if cursor.is_null() {
            break;
        }
    }
    let mut expected = vec![json!(thread), json!(second_thread)];
    expected.sort_by_key(Value::to_string);
    threads.sort_by_key(Value::to_string);
    assert_eq!(threads, expected);
    assert!(cursor.is_null());
    Ok(())
}

fn thread_ids(case: &PagingCase) -> Vec<Value> {
    let mut expected = vec![json!(case.thread), json!(case.second_thread)];
    expected.sort_by_key(Value::to_string);
    expected
}

async fn watch_and_unwatch(fixture: &BoardFixture, case: &PagingCase) -> FixtureResult {
    let PagingCase {
        thread,
        second,
        second_thread,
        reader,
        ..
    } = case;
    let expected = thread_ids(case);
    for (project, thread) in [(&fixture.project, thread), (second, second_thread)] {
        accepted(at(
            &fixture
                .cli(&[
                    "thread",
                    "watch",
                    "--thread-id",
                    &format!("{project}/{thread}"),
                    "--actor",
                    &reader.to_string(),
                ])
                .await?,
            "/record",
        )?)?;
    }
    let watched = fixture
        .cli(&[
            "thread",
            "list",
            "--workspace-id",
            &fixture.workspace,
            "--reader",
            &reader.to_string(),
            "--watched-only",
        ])
        .await?;
    let mut observed = records(&watched, "/threadId")?;
    observed.sort_by_key(Value::to_string);
    assert_eq!(observed, expected);
    fixture
        .accept(
            "thread_unwatch",
            fields(
                header(reader),
                json!({"thread":{"project":fixture.project,"thread":thread}}),
            )?,
        )
        .await?;
    let unwatched = fixture
        .cli(&[
            "thread",
            "list",
            "--workspace-id",
            &fixture.workspace,
            "--reader",
            &reader.to_string(),
            "--watched-only",
        ])
        .await?;
    assert_eq!(
        records(&unwatched, "/threadId")?,
        vec![json!(second_thread)]
    );
    Ok(())
}

async fn isolate_corrupt_sibling(fixture: &mut BoardFixture, case: &PagingCase) -> FixtureResult {
    let PagingCase {
        thread,
        reader,
        posts,
        ..
    } = case;
    let expected = thread_ids(case);
    let (_, foreign) = fixture.foreign_project().await?;
    let corrupt_thread = fixture.thread(&foreign, &fixture.owner).await?;
    fixture.stop_host().await?;
    super::records_reopen::damage_thread(fixture, &foreign, &corrupt_thread).await?;
    fixture.reopen_host().await?;
    let roots = fixture
        .host()?
        .mcp(
            "thread_list",
            json!({"workspaceId":fixture.workspace,"reader":reader,
        "watchedOnly":false,"page":{"limit":100,"cursor":null}}),
        )
        .await?;
    let mut observed = records(&roots, "/threadId")?;
    observed.sort_by_key(Value::to_string);
    assert_eq!(observed, expected);
    let healthy = fixture
        .host()?
        .mcp(
            "post_list",
            json!({"scope":{"kind":"thread","thread":{"project":fixture.project,"thread":thread}},
        "selection":{"kind":"latest"},"page":{"limit":100,"cursor":null}}),
        )
        .await?;
    let expected = posts
        .iter()
        .rev()
        .map(|post| Ok(json!([at(post, "/postId")?, at(post, "/text")?])))
        .chain(std::iter::once(Ok(json!([
            thread,
            format!("root {thread}")
        ]))))
        .collect::<FixtureResult<Vec<_>>>()?;
    let actual = at(&healthy, "/page/records")?
        .as_array()
        .ok_or("post rows")?
        .iter()
        .map(|post| Ok(json!([at(post, "/postId")?, at(post, "/text")?])))
        .collect::<FixtureResult<Vec<_>>>()?;
    assert_eq!(actual, expected);
    let corrupt=fixture.host()?.mcp_reply("thread_show",json!({"thread":{"project":foreign,"thread":corrupt_thread},"reader":{"kind":"anonymous"}})).await?;
    assert!(corrupt.is_error);
    assert_eq!(
        json!([
            corrupt.value.get("mcpResult"),
            corrupt.value.get("stage"),
            corrupt.value.get("kind"),
            corrupt.value.pointer("/details/resource/threadId")
        ]),
        json!(["error", "inspection", "invalidRecord", corrupt_thread])
    );
    Ok(())
}

async fn archive_workspace(fixture: &BoardFixture) -> FixtureResult {
    accepted(at(
        &fixture
            .cli(&[
                "workspace",
                "archive",
                "--workspace-id",
                &fixture.workspace,
                "--actor",
                &fixture.owner.to_string(),
            ])
            .await?,
        "/record",
    )?)?;
    // Unlike the old launcher stand-in, this Host has real writers that can acknowledge archive.
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            let progress = machine_progress::observe(fixture.host()?, None).await?;
            let workspace = fixture
                .host()?
                .mcp("workspace_show", json!({"workspaceId":fixture.workspace}))
                .await?;
            if workspace.pointer("/workspace/state") == Some(&json!("archived")) {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
            }
            machine_progress::observe(fixture.host()?, Some(progress)).await?;
        }
    })
    .await??;
    Ok(())
}
