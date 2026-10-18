//! Repository and implicit CLI lists preserve the service's current subject projection.
#[path = "support/board_cli_execution.rs"]
mod board_cli_execution;
#[path = "../../collaboration-service/tests/support/board_fixture.rs"]
pub mod board_fixture;

use board_cli_execution::cli;
use board_fixture::{BoardFixture, TestError, accepted};
use message_board::*;
use message_board_storage::FleetCache;
use serde_json::{Value, json};

const ROOT_BODY: &str = "Discussion body, deliberately different from every subject title";

struct ExpectedThread {
    project: ProjectId,
    thread: ThreadId,
    subject: ThreadSubjectView,
    imported: bool,
}

async fn attach_repository(fixture: &BoardFixture) -> Result<(), TestError> {
    // Discovery reads Git metadata only; the association belongs to the isolated service.
    let repository = std::env::current_dir()?;
    board_cli_execution::accepted(
        &cli(
            fixture._directory.path(),
            &[
                "repository",
                "attach",
                "--workspace-id",
                fixture.workspace.as_str(),
                "--repository-path",
                repository.to_str().ok_or("repository path")?,
                "--actor",
                &fixture.actor.to_string(),
            ],
        )
        .await?,
    );
    Ok(())
}

async fn create_thread(
    fixture: &BoardFixture,
    project: &ProjectId,
    subject: ThreadSubject,
) -> Result<ExpectedThread, TestError> {
    let post = PostId::generate();
    let mut request = fixture.header();
    request.as_object_mut().ok_or("header")?.extend(
        json!({"projectId":project,"observedEpoch":1,"postId":post,"about":subject,
            "text":ROOT_BODY,"references":[],"addressees":[],"role":"orchestrator","watch":true})
        .as_object()
        .ok_or("thread fields")?
        .clone(),
    );
    accepted(&fixture.api.call("thread_create", request).await?);
    Ok(ExpectedThread {
        project: project.clone(),
        thread: post.into(),
        subject: ThreadSubjectView {
            about: subject,
            title: SubjectTitle::Unavailable,
        },
        imported: false,
    })
}

fn listed_thread<'a>(response: &'a Value, thread: &ThreadId) -> Result<&'a Value, TestError> {
    response
        .pointer("/result/page/records")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter()
                .find(|row| row.get("threadId") == Some(&json!(thread)))
        })
        .ok_or_else(|| format!("missing thread {}: {response}", thread.as_str()).into())
}

async fn assert_list_parity(
    fixture: &BoardFixture,
    expected: &ExpectedThread,
    protected_text: &[&str],
) -> Result<(), TestError> {
    let directory = fixture._directory.path();
    let reader = fixture.actor.to_string();
    let repository = std::env::current_dir()?;
    let workspace = cli(
        directory,
        &[
            "thread",
            "list",
            "--workspace-id",
            fixture.workspace.as_str(),
            "--reader",
            &reader,
        ],
    )
    .await?;
    let mcp = fixture.api.call("thread_list", json!({"workspaceId":fixture.workspace,"reader":fixture.actor,"watchedOnly":false,"page":{"limit":100,"cursor":null}})).await?;
    let explicit = cli(
        directory,
        &[
            "thread",
            "list",
            "--repository-path",
            repository.to_str().ok_or("repository path")?,
            "--reader",
            &reader,
        ],
    )
    .await?;
    let implicit = cli(directory, &["thread", "list", "--reader", &reader]).await?;
    let anonymous = cli(directory, &["thread", "list"]).await?;
    let watch = fixture.api.call("thread_show", json!({"thread":{"thread":expected.thread,"project":expected.project},"reader":{"kind":"identified","reader":fixture.actor}})).await?;
    assert_eq!(
        watch.pointer("/result/watchStatus/watching"),
        Some(&json!(true)),
        "{watch}"
    );
    let subject = serde_json::to_value(&expected.subject)?;
    let mcp_row = listed_thread(&mcp, &expected.thread)?;
    assert_eq!(mcp_row.get("subject"), Some(&subject), "{mcp}");
    assert_eq!(mcp_row.get("imported"), Some(&json!(expected.imported)));
    assert_eq!(listed_thread(&workspace, &expected.thread)?, mcp_row);
    for (selector, response, identified) in [
        ("repository-path", &explicit, true),
        ("no-selector", &implicit, true),
        ("plain thread list", &anonymous, false),
    ] {
        let row = listed_thread(response, &expected.thread)?;
        assert_eq!(
            row.get("subject"),
            Some(&subject),
            "{selector} dropped the current subject view: {response}"
        );
        assert!(
            row.get("title").is_none(),
            "{selector} retained a copied body title: {row}"
        );
        assert_eq!(row.get("imported"), Some(&json!(expected.imported)));
        assert_eq!(row.get("projectId"), Some(&json!(expected.project)));
        assert_eq!(row.get("workspaceId"), Some(&json!(fixture.workspace)));
        assert_eq!(row.get("boardId"), Some(&json!(fixture.board)));
        assert_eq!(row.get("orchestrator"), mcp_row.get("orchestrator"));
        assert_eq!(row.get("implementer"), mcp_row.get("implementer"));
        let watch_status = if identified {
            watch.pointer("/result/watchStatus")
        } else {
            Some(&Value::Null)
        };
        assert_eq!(row.get("watchStatus"), watch_status);
        assert!(
            row.get("lastActivity")
                .is_some_and(|value| !value.is_null())
        );
    }
    for text in protected_text {
        for response in [&workspace, &mcp, &explicit, &implicit, &anonymous] {
            assert!(
                !response.to_string().contains(text),
                "protected subject leaked: {response}"
            );
        }
    }
    Ok(())
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Literal contract assertions; Result propagates fixture failures.
async fn repository_and_default_lists_follow_subject_rename_and_refile() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    attach_repository(&fixture).await?;
    let (project, stream) = fixture.project().await?;
    let mut expected = create_thread(
        &fixture,
        &project,
        ThreadSubject::Workstream(stream.clone()),
    )
    .await?;
    expected.subject.title = SubjectTitle::Shown("work".into());
    assert_list_parity(&fixture, &expected, &[]).await?;

    accepted(&fixture.api.call("workstream_rename", fixture.request(&project, json!({"kind":"workstreamRename","workstreamId":stream,"name":"Release review","expectedRevision":0}))).await?);
    expected.subject.title = SubjectTitle::Shown("Release review".into());
    assert_list_parity(&fixture, &expected, &[]).await?;

    let about = ThreadSubject::Project {
        line: "Refiled repository subject".to_owned().try_into()?,
    };
    let mut request = fixture.header();
    request.as_object_mut().ok_or("header")?.extend(json!({"observedEpoch":1,"thread":{"thread":expected.thread,"project":project},"about":about}).as_object().ok_or("refile fields")?.clone());
    accepted(&fixture.api.call("thread_refile", request).await?);
    expected.subject = ThreadSubjectView {
        about,
        title: SubjectTitle::Shown("Refiled repository subject".into()),
    };
    assert_list_parity(&fixture, &expected, &[]).await?;
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn repository_and_default_lists_keep_redacted_subjects_private() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    attach_repository(&fixture).await?;
    let (project, _) = fixture.project().await?;
    let row = WorkTrailRowId::generate();
    const PRIVATE: &str = "Protected row first line";
    accepted(&fixture.api.call("work_trail_add", fixture.request(&project, json!({"kind":"workTrailAdd","rowId":row,"subject":{"kind":"project"},"text":format!("{PRIVATE}\nProtected detail"),"tags":[],"links":[]}))).await?);
    let mut expected =
        create_thread(&fixture, &project, ThreadSubject::WorkTrailRow(row.clone())).await?;
    accepted(
        &fixture
            .api
            .call(
                "work_trail_redact",
                fixture.request(
                    &project,
                    json!({"kind":"workTrailRedact","rowId":row,"redacted":true}),
                ),
            )
            .await?,
    );
    expected.subject.title = SubjectTitle::Redacted;
    assert_list_parity(&fixture, &expected, &[PRIVATE, "Protected detail"]).await?;
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn repository_and_default_lists_keep_unavailable_subjects_and_imported_marker()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    attach_repository(&fixture).await?;
    let (project, _) = fixture.project().await?;
    let adr = AdrId::generate();
    const PRIVATE: &str = "Protected cached ADR title";
    let mut request = fixture.header();
    request.as_object_mut().ok_or("header")?.extend(json!({"workspaceId":fixture.workspace,"adrId":adr,"title":PRIVATE,"text":"Protected ADR body","supersedes":null,"madeIn":null,"repositoryFiles":[]}).as_object().ok_or("ADR fields")?.clone());
    accepted(&fixture.api.call("adr_create", request).await?);
    let mut expected =
        create_thread(&fixture, &project, ThreadSubject::WorkspaceAdr(adr.clone())).await?;

    // Stored imported-thread input stands in for M5's upgrade, not for the read path.
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project store")?;
    let mut write = store.begin_admission().await?;
    let Some(Record::Thread(mut imported)) = write.record(&expected.thread.clone().into()).await?
    else {
        return Err("stored thread".into());
    };
    imported.imported = true;
    write.append_record(Record::Thread(imported)).await?;
    write.commit().await?;
    expected.imported = true;

    let cache = FleetCache::open(&fixture._directory.path().join("fleet-cache.sqlite")).await?;
    let mut snapshot = cache.snapshot().await?;
    let before = snapshot.records.len();
    snapshot
        .records
        .retain(|record| !matches!(record, Record::Adr(record) if record.adr_id == adr));
    assert_eq!(
        snapshot.records.len() + 1,
        before,
        "remove exactly the actual cached ADR"
    );
    cache.replace_snapshot(&snapshot).await?;
    assert!(cache.record(&adr.into()).await?.is_none());
    cache.close().await;
    assert_list_parity(&fixture, &expected, &[PRIVATE, "Protected ADR body"]).await?;
    fixture.stop().await
}
