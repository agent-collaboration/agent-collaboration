//! V8: executable CLI and MCP tools share role/subject admission and replay decisions.
#[path = "support/board_cli_execution.rs"]
mod board_cli_execution;
#[expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "This fixture source belongs to the split lane, which fixes its allowances at integration."
)]
#[path = "../../collaboration-service/tests/support/board_fixture.rs"]
pub mod board_fixture;
use board_cli_execution::cli;
use board_fixture::{BoardFixture, TestError};
use message_board::*;
use serde_json::{Value, json};

async fn replay_role(
    fixture: &BoardFixture,
    tool: &str,
    subcommand: &str,
    request: Value,
) -> Result<Value, TestError> {
    let path = fixture._directory.path().join("role-parity.json");
    std::fs::write(&path, serde_json::to_vec(&request)?)?;
    let from_cli = cli(
        fixture._directory.path(),
        &[
            "role",
            subcommand,
            "--request-file",
            path.to_str().ok_or("path")?,
        ],
    )
    .await?;
    board_cli_execution::accepted(&from_cli);
    let from_mcp = fixture.api.call(tool, request).await?;
    assert_eq!(
        from_cli.pointer("/result/record/result"),
        from_mcp.pointer("/result/result"),
        "{from_cli} != {from_mcp}"
    );
    assert_eq!(
        from_mcp.pointer("/result/result/decision/kind"),
        Some(&json!("accepted")),
        "{from_mcp}"
    );
    Ok(from_mcp)
}

#[tokio::test]

async fn six_role_names_and_holder_qualified_releases_match_cli_and_mcp() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, stream) = fixture.project().await?;
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    for (role, subject) in [
        ("lead", RoleSubject::Project(project.clone())),
        ("advisor", RoleSubject::Project(project.clone())),
        ("implementer", RoleSubject::Workstream(stream.clone())),
        ("verifier", RoleSubject::Workstream(stream.clone())),
        ("reviewer", RoleSubject::Workstream(stream.clone())),
        ("operator", RoleSubject::Workstream(stream.clone())),
    ] {
        let mut read = store.snapshot().await?;
        let revision = read.assignment_revision(&subject).await?;
        read.finish().await?;
        let holder = if role == "lead" {
            fixture.actor.clone()
        } else {
            json!({"kind":"human","humanId":format!("surface-{role}")})
        };
        let granted = replay_role(&fixture, "role_grant", "grant", fixture.request(&project, json!({"kind":"roleGrant","subject":subject,"role":role,"holder":holder,"model":null,"lineage":null,"expectedRevision":revision,"ownerGrant":null}))).await?;
        if ["advisor", "reviewer", "operator"].contains(&role) {
            let reference: RecordRef = serde_json::from_value(
                granted
                    .pointer("/result/result/decision/effects/entries/0/0")
                    .ok_or("assignment reference")?
                    .clone(),
            )?;
            let mut read = store.snapshot().await?;
            let Some(Record::RoleAssignment(assignment)) = read.record(&reference).await? else {
                return Err("assignment body".into());
            };
            let revision = read.assignment_revision(&subject).await?;
            read.finish().await?;
            replay_role(&fixture, "role_release", "release", fixture.request(&project, json!({"kind":"roleRelease","subject":subject,"role":role,"holder":assignment.holder,"expectedRevision":revision}))).await?;
        }
    }
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn every_about_grammar_and_refile_match_cli_mcp_and_current_subject_reads()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, stream) = fixture.project().await?;
    let task = fixture.task(&project, &stream).await?;
    let row = WorkTrailRowId::generate();
    board_fixture::accepted(&fixture.api.call("work_trail_add", fixture.request(&project, json!({"kind":"workTrailAdd","rowId":row,"subject":{"kind":"project"},"text":"Surface row\nDetail","tags":[],"links":[]}))).await?);
    let adr = AdrId::generate();
    let mut adr_request = fixture.header();
    adr_request.as_object_mut().ok_or("header")?.extend(json!({"workspaceId":fixture.workspace,"adrId":adr,"title":"Surface ADR","text":"Decision","supersedes":null,"madeIn":null,"repositoryFiles":[]}).as_object().ok_or("fields")?.clone());
    board_fixture::accepted(&fixture.api.call("adr_create", adr_request).await?);
    let text_file = fixture._directory.path().join("surface-root.md");
    std::fs::write(&text_file, "Surface discussion")?;
    for (index, (grammar, subject, title)) in [
        (
            "project:Surface topic".to_owned(),
            json!({"kind":"project","subject":{"line":"Surface topic"}}),
            "Surface topic",
        ),
        (
            format!("workstream:{}", stream.as_str()),
            json!({"kind":"workstream","subject":stream}),
            "work",
        ),
        (
            format!("task:{}", task.as_str()),
            json!({"kind":"task","subject":task}),
            "task",
        ),
        (
            format!("row:{}", row.as_str()),
            json!({"kind":"workTrailRow","subject":row}),
            "Surface row",
        ),
        (
            format!("adr:{}", adr.as_str()),
            json!({"kind":"workspaceAdr","subject":adr}),
            "Surface ADR",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let request_id = RequestId::generate();
        let post = PostId::generate();
        let actor = if index == 0 {
            fixture.actor.clone()
        } else {
            json!({"kind":"human","humanId":format!("surface-author-{index}")})
        };
        let actor_argument = serde_json::to_string(&actor)?;
        let from_cli = cli(
            fixture._directory.path(),
            &[
                "thread",
                "create",
                "--project-id",
                project.as_str(),
                "--post-id",
                post.as_str(),
                "--request-id",
                request_id.as_str(),
                "--about",
                &grammar,
                "--actor",
                &actor_argument,
                "--no-watch",
                "--text-file",
                text_file.to_str().ok_or("text path")?,
            ],
        )
        .await?;
        let request = json!({"requestId":request_id,"protocolVersion":1,"actingFor":null,"actor":actor,"projectId":project,"postId":post,"about":subject,"text":"Surface discussion","references":[],"addressees":[],"role":null,"watch":false});
        let from_mcp = fixture.api.call("thread_create", request).await?;
        assert_eq!(
            from_cli.pointer("/result/record/result"),
            from_mcp.pointer("/result/result"),
            "{from_cli} != {from_mcp}"
        );
        board_fixture::accepted(&from_mcp);
        let thread = ThreadRef {
            thread: ThreadId::from(post),
            project: Some(project.clone()),
        };
        let thread_argument = format!("{}/{}", project.as_str(), thread.thread.as_str());
        let shown = cli(
            fixture._directory.path(),
            &["thread", "show", "--thread-id", &thread_argument],
        )
        .await?;
        assert_eq!(
            shown.pointer("/result/record/subject/title/title"),
            Some(&json!(title)),
            "{shown}"
        );
        let refile_id = RequestId::generate();
        let refile_subject = json!({"kind":"project","subject":{"line":"Refiled surface topic"}});
        let owner = serde_json::to_string(&fixture.actor)?;
        let from_cli = cli(
            fixture._directory.path(),
            &[
                "thread",
                "refile",
                "--thread-id",
                &thread_argument,
                "--about",
                "project:Refiled surface topic",
                "--actor",
                &owner,
                "--request-id",
                refile_id.as_str(),
            ],
        )
        .await?;
        let from_mcp = fixture.api.call("thread_refile", json!({"requestId":refile_id,"protocolVersion":1,"actor":fixture.actor,"actingFor":null,"thread":thread,"about":refile_subject})).await?;
        assert_eq!(
            from_cli.pointer("/result/record/result"),
            from_mcp.pointer("/result/result"),
            "{from_cli} != {from_mcp}"
        );
        board_fixture::accepted(&from_mcp);
        let shown = fixture
            .api
            .call(
                "thread_show",
                json!({"thread":thread,"reader":{"kind":"anonymous"}}),
            )
            .await?;
        assert_eq!(
            shown.pointer("/result/subject/about"),
            Some(&refile_subject)
        );
    }
    fixture.stop().await
}
