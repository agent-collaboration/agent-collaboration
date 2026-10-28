//! BR14: repository defaults preserve computed subjects and omit protected source text.
#![deny(clippy::too_many_lines)]
use super::fixture::{BoardFixture, accepted, at, fields, header, human, identity};
use super::repository_fixture::{attach_repository, repository_cli};
use codex_router_test_support::FixtureResult;
use serde_json::{Value, json};
use sqlx_turso::sqlx::{ConnectOptions, Connection, Row};
use std::path::Path;

#[derive(Clone, Copy)]
enum ListShape {
    Workspace,
    IdentifiedRepository,
    AnonymousRepository,
}

struct ListedThread {
    id: String,
    author: Value,
    subject: Value,
    last_activity: Value,
    root_body: String,
}

pub async fn repository_defaults_and_privacy(fixture: &mut BoardFixture) -> FixtureResult {
    fixture.create().await?;
    let repository = attach_repository(fixture).await?;
    let mut threads = initial_and_refiled_subject(fixture, &repository).await?;
    let (row, adr) = add_private_subjects(fixture, &repository, &mut threads).await?;
    fixture
        .accept(
            "work_trail_redact",
            fixture.request(
                &fixture.owner,
                json!({"kind":"workTrailRedact",
        "rowId":row,"redacted":true}),
            ),
        )
        .await?;
    threads.get_mut(1).ok_or("row thread absent")?.subject =
        json!({"about":{"kind":"workTrailRow","subject":row},"title":{"kind":"redacted"}});
    let feed = fixture.feed().await?;
    remove_cached_adr_offline(fixture, &adr).await?;
    threads.get_mut(2).ok_or("ADR thread absent")?.subject =
        json!({"about":{"kind":"workspaceAdr","subject":adr},"title":{"kind":"unavailable"}});
    assert_eq!(
        fixture.feed().await?,
        feed,
        "cache fault changed the actual project feed"
    );
    observe_lists(
        fixture,
        &repository,
        &threads,
        &[
            "protected row title",
            "protected row detail",
            "protected ADR title",
            "protected ADR body",
        ],
    )
    .await
}

async fn initial_and_refiled_subject(
    fixture: &BoardFixture,
    repository: &Path,
) -> FixtureResult<Vec<ListedThread>> {
    let workstream_about = json!({"kind":"workstream","subject":fixture.workstream});
    let mut threads = vec![
        create_thread(
            fixture,
            workstream_about.clone(),
            json!({"kind":"shown","title":"verification lane"}),
            fixture.owner.clone(),
            "workstream",
        )
        .await?,
    ];
    observe_lists(fixture, repository, &threads, &[]).await?;
    fixture
        .accept(
            "workstream_rename",
            fixture.request(
                &fixture.owner,
                json!({"kind":"workstreamRename",
        "workstreamId":fixture.workstream,"name":"current list subject name","expectedRevision":0}),
            ),
        )
        .await?;
    let first = threads.first_mut().ok_or("workstream thread absent")?;
    first.subject = json!({"about":workstream_about,"title":{"kind":"shown","title":"current list subject name"}});
    observe_lists(fixture, repository, &threads, &[]).await?;
    let first_id = threads
        .first()
        .ok_or("workstream thread absent")?
        .id
        .clone();
    let refiled = json!({"kind":"project","subject":{"line":"refiled repository list subject"}});
    accepted(at(
        &repository_cli(
            fixture,
            repository,
            &[
                "thread".into(),
                "refile".into(),
                "--thread-id".into(),
                format!("{}/{}", fixture.project, first_id),
                "--actor".into(),
                fixture.owner.to_string(),
                "--about".into(),
                "project:refiled repository list subject".into(),
            ],
        )
        .await?,
        "/record",
    )?)?;
    let events = fixture.feed_records("threadRefiled").await?;
    assert_eq!(events.len(), 1);
    let event = events.first().ok_or("refile fact absent")?;
    assert_eq!(event.get("from"), Some(&workstream_about));
    assert_eq!(event.get("to"), Some(&refiled));
    threads.first_mut().ok_or("refiled thread absent")?.subject = json!({"about":refiled,
        "title":{"kind":"shown","title":"refiled repository list subject"}});
    observe_lists(fixture, repository, &threads, &[]).await?;
    Ok(threads)
}

async fn add_private_subjects(
    fixture: &BoardFixture,
    repository: &Path,
    threads: &mut Vec<ListedThread>,
) -> FixtureResult<(String, String)> {
    let row = identity();
    let adr = identity();
    fixture.accept("work_trail_add", fixture.request(&fixture.owner,json!({"kind":"workTrailAdd",
        "rowId":row,"subject":{"kind":"project"},"text":"protected row title\nprotected row detail","tags":[],"links":[]}))).await?;
    threads.push(
        create_thread(
            fixture,
            json!({"kind":"workTrailRow","subject":row}),
            json!({"kind":"shown","title":"protected row title"}),
            human("list-row-author"),
            "row",
        )
        .await?,
    );
    fixture
        .accept(
            "adr_create",
            fields(
                header(&fixture.owner),
                json!({"workspaceId":fixture.workspace,
        "adrId":adr,"title":"protected ADR title","text":"protected ADR body",
        "supersedes":null,"madeIn":null,"repositoryFiles":[]}),
            )?,
        )
        .await?;
    threads.push(
        create_thread(
            fixture,
            json!({"kind":"workspaceAdr","subject":adr}),
            json!({"kind":"shown","title":"protected ADR title"}),
            human("list-adr-author"),
            "ADR",
        )
        .await?,
    );
    observe_lists(fixture, repository, threads, &[]).await?;
    Ok((row, adr))
}

async fn create_thread(
    fixture: &BoardFixture,
    about: Value,
    title: Value,
    author: Value,
    label: &str,
) -> FixtureResult<ListedThread> {
    let id = identity();
    let body = format!("private root BODY {label}; unlike its computed subject title");
    fixture.accept("thread_create", fields(header(&author),json!({"projectId":fixture.project,
        "postId":id,"about":about,"text":body,"references":[],"addressees":[],"role":"orchestrator","watch":true}))?).await?;
    fixture
        .accept(
            "thread_join",
            fields(
                json!({"requestId":super::fixture::identity(),"protocolVersion":1,"actor":&fixture.helper}),
                json!({"thread":{"project":fixture.project,"thread":id},
        "role":"implementer","watch":true,"replace":null,"note":null}),
            )?,
        )
        .await?;
    fixture
        .accept(
            "thread_watch",
            fields(
                header(&fixture.owner),
                json!({"thread":{"project":fixture.project,"thread":id}}),
            )?,
        )
        .await?;
    let mut reply = fixture.reply_request(
        &fixture.project,
        &id,
        &format!("known last activity {label}"),
    )?;
    reply
        .as_object_mut()
        .ok_or("reply request absent")?
        .insert("actor".into(), fixture.helper.clone());
    let post = at(&reply, "/postId")?.clone();
    fixture.accept("thread_post", reply).await?;
    let shown = fixture
        .host()?
        .mcp("post_show", json!({"postId":post}))
        .await?;
    Ok(ListedThread {
        id,
        author,
        subject: json!({"about":about,"title":title}),
        last_activity: at(&shown, "/position")?.clone(),
        root_body: body,
    })
}

async fn observe_lists(
    fixture: &BoardFixture,
    repository: &Path,
    expected: &[ListedThread],
    protected: &[&str],
) -> FixtureResult {
    let mcp = fixture
        .host()?
        .mcp(
            "thread_list",
            json!({"workspaceId":fixture.workspace,"reader":fixture.owner,
        "watchedOnly":false,"page":{"limit":100,"cursor":null}}),
        )
        .await?;
    let reader = fixture.owner.to_string();
    let variants = [
        (
            "workspace",
            vec![
                "--workspace-id".into(),
                fixture.workspace.clone(),
                "--reader".into(),
                reader.clone(),
            ],
            ListShape::Workspace,
        ),
        (
            "repository",
            vec![
                "--repository-path".into(),
                repository.to_str().ok_or("repository path")?.into(),
                "--reader".into(),
                reader.clone(),
            ],
            ListShape::IdentifiedRepository,
        ),
        (
            "current",
            vec!["--reader".into(), reader],
            ListShape::IdentifiedRepository,
        ),
        ("plain", vec![], ListShape::AnonymousRepository),
    ];
    assert_list(fixture, &mcp, expected, protected, ListShape::Workspace).await?;
    for (name, arguments, shape) in variants {
        let mut command = vec!["thread".into(), "list".into()];
        command.extend(arguments);
        let reply = repository_cli(fixture, repository, &command).await?;
        if name == "workspace" {
            assert_eq!(reply, mcp);
        }
        assert_list(fixture, &reply, expected, protected, shape).await?;
    }
    Ok(())
}

async fn assert_list(
    fixture: &BoardFixture,
    reply: &Value,
    expected: &[ListedThread],
    protected: &[&str],
    shape: ListShape,
) -> FixtureResult {
    let rows = at(reply, "/page/records")?
        .as_array()
        .ok_or("thread list rows absent")?;
    let mut actual = rows
        .iter()
        .map(|row| at(row, "/threadId").cloned())
        .collect::<FixtureResult<Vec<_>>>()?;
    let mut identities = expected
        .iter()
        .map(|thread| json!(thread.id))
        .collect::<Vec<_>>();
    actual.sort_by_key(Value::to_string);
    identities.sort_by_key(Value::to_string);
    assert_eq!(actual, identities);
    assert_eq!(reply.pointer("/page/nextCursor"), Some(&Value::Null));
    let serialized = reply.to_string();
    for text in expected
        .iter()
        .map(|thread| thread.root_body.as_str())
        .chain(protected.iter().copied())
    {
        assert!(
            !serialized.contains(text),
            "reduced list leaked protected source text"
        );
    }
    for thread in expected {
        let row = rows
            .iter()
            .find(|row| row.get("threadId") == Some(&json!(thread.id)))
            .ok_or("expected thread absent")?;
        assert_eq!(row.get("subject"), Some(&thread.subject));
        assert_eq!(row.get("imported"), Some(&json!(false)));
        assert!(
            !row.as_object()
                .ok_or("thread row shape")?
                .contains_key("title")
        );
        assert_eq!(row.pointer("/orchestrator/identity"), Some(&thread.author));
        assert_eq!(row.pointer("/implementer/identity"), Some(&fixture.helper));
        assert_eq!(row.get("projectId"), Some(&json!(fixture.project)));
        let shown = fixture
            .host()?
            .mcp(
                "thread_show",
                json!({"thread":{"project":fixture.project,"thread":thread.id},
            "reader":{"kind":"identified","reader":fixture.owner}}),
            )
            .await?;
        assert_eq!(shown.pointer("/watchStatus/watching"), Some(&json!(true)));
        assert_list_metadata(fixture, row, thread, &shown, shape);
    }
    Ok(())
}

fn assert_list_metadata(
    fixture: &BoardFixture,
    row: &Value,
    thread: &ListedThread,
    shown: &Value,
    shape: ListShape,
) {
    match shape {
        ListShape::Workspace => {
            assert_eq!(
                row.pointer("/envelope/scope/workspace"),
                Some(&json!(fixture.workspace))
            );
            assert_eq!(
                row.pointer("/envelope/scope/board"),
                Some(&json!(fixture.board))
            );
        }
        ListShape::IdentifiedRepository => {
            assert_repository_metadata(fixture, row, thread);
            assert_eq!(row.get("watchStatus"), shown.get("watchStatus"));
        }
        ListShape::AnonymousRepository => {
            assert_repository_metadata(fixture, row, thread);
            assert_eq!(row.get("watchStatus"), Some(&Value::Null));
        }
    }
}

fn assert_repository_metadata(fixture: &BoardFixture, row: &Value, thread: &ListedThread) {
    assert_eq!(row.get("lastActivity"), Some(&thread.last_activity));
    assert_eq!(row.get("workspaceId"), Some(&json!(fixture.workspace)));
    assert_eq!(row.get("boardId"), Some(&json!(fixture.board)));
}

async fn remove_cached_adr_offline(fixture: &mut BoardFixture, adr: &str) -> FixtureResult {
    let stopped = fixture
        .host
        .take()
        .ok_or("ADR fault Host absent")?
        .stop_for_readback()
        .await?;
    fixture
        .central
        .take()
        .ok_or("ADR fault central absent")?
        .shutdown()
        .await?;
    // Stop the actual central, so startup cannot refill this one omitted cache row.
    let path = stopped
        .directory()
        .join("fleet-cache.sqlite")
        .canonicalize()?;
    if !path.starts_with(stopped.directory()) {
        return Err("ADR cache escaped the owned directory".into());
    }
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(path)
        .create_if_missing(false)
        .connect()
        .await?;
    let result = async {
        let before = cache_rows(&mut connection).await?;
        let expected = before
            .iter()
            .filter(|row| !(row.0 == "adr" && row.1 == adr))
            .cloned()
            .collect::<Vec<_>>();
        if expected.len() + 1 != before.len() {
            return Err("fault must remove exactly one real admitted ADR".into());
        }
        let changed = sqlx_turso::sqlx::query::<sqlx_turso::Turso>(
            "DELETE FROM cached_fleet_records WHERE kind='adr' AND id=?",
        )
        .bind(adr)
        .execute(&mut connection)
        .await?;
        if changed.rows_affected() != 1 {
            return Err("ADR fault did not remove exactly one row".into());
        }
        if cache_rows(&mut connection).await? != expected {
            return Err("ADR fault changed unrelated cache rows".into());
        }
        Ok(())
    }
    .await;
    let closed = connection.close().await;
    super::stopped_faults::finish_stopped_access(result, closed.map_err(Into::into))?;
    fixture.reopen_host().await
}

async fn cache_rows(
    connection: &mut sqlx_turso::TursoConnection,
) -> FixtureResult<Vec<(String, String, String)>> {
    sqlx_turso::sqlx::query::<sqlx_turso::Turso>(
        "SELECT json_array(kind,id,payload) AS value FROM cached_fleet_records ORDER BY kind,id",
    )
    .fetch_all(connection)
    .await?
    .iter()
    .map(|row| {
        let value: String = row.try_get("value")?;
        Ok(serde_json::from_str(&value)?)
    })
    .collect()
}
