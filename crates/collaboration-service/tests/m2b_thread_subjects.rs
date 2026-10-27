#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! TH-1…TH-3 and the root-entry cutover through real MCP/native admission.
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
use board_fixture::{BoardFixture, TestError, accepted, refusal_kind};
use message_board::*;
use serde_json::{Value, json};
use sqlx::{ConnectOptions, Connection};

fn create(
    fixture: &BoardFixture,
    project: &ProjectId,
    about: Option<Value>,
) -> Result<Value, TestError> {
    let mut request = fixture.header();
    let fields = request.as_object_mut().ok_or("header")?;
    fields.extend(json!({"projectId":project,"postId":PostId::generate(),"text":"discussion","references":[],"role":null,"watch":false,"addressees":[]}).as_object().ok_or("fields")?.clone());
    if let Some(about) = about {
        fields.insert("about".to_owned(), about);
    }
    Ok(request)
}

fn project_subject(line: &str) -> Value {
    json!({"kind":"project","subject":{"line":line}})
}
fn thread_of(request: &Value) -> Value {
    json!({"thread":request["postId"],"project":request["projectId"]})
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; Result propagates fixture errors."
)]
async fn every_subject_variant_is_required_and_read_titles_follow_current_records()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    for variant in [
        "project",
        "workstream",
        "task",
        "workTrailRow",
        "workspaceAdr",
    ] {
        let (project, workstream) = fixture.project().await?;
        let (subject, title) = match variant {
            "project" => (project_subject("Review the plan"), "Review the plan"),
            "workstream" => (json!({"kind":"workstream","subject":workstream}), "work"),
            "task" => (
                json!({"kind":"task","subject":fixture.task(&project, &workstream).await?}),
                "task",
            ),
            "workTrailRow" => {
                let row = WorkTrailRowId::generate();
                accepted(&fixture.api.call("work_trail_add", fixture.request(&project, json!({"kind":"workTrailAdd","rowId":row,"subject":{"kind":"project"},"text":"First line\nMore detail","tags":[],"links":[]}))).await?);
                (json!({"kind":"workTrailRow","subject":row}), "First line")
            }
            "workspaceAdr" => {
                let adr = AdrId::generate();
                let mut request = fixture.header();
                request.as_object_mut().ok_or("header")?.extend(json!({"workspaceId":fixture.workspace,"adrId":adr,"title":"Decision title","text":"Decision text","supersedes":null,"madeIn":null,"repositoryFiles":[]}).as_object().ok_or("adr fields")?.clone());
                accepted(&fixture.api.call("adr_create", request).await?);
                (
                    json!({"kind":"workspaceAdr","subject":adr}),
                    "Decision title",
                )
            }
            _ => return Err("subject case".into()),
        };
        let mut request = create(&fixture, &project, Some(subject.clone()))?;
        if variant != "project" {
            request["actor"] = json!({"kind":"human","humanId":"no-project-role"});
        }
        accepted(&fixture.api.call("thread_create", request.clone()).await?);
        let shown = fixture
            .api
            .call(
                "thread_show",
                json!({"thread":thread_of(&request),"reader":{"kind":"anonymous"}}),
            )
            .await?;
        assert_eq!(shown.pointer("/result/about"), Some(&subject), "{shown}");
        assert_eq!(shown.pointer("/result/imported"), Some(&json!(false)));
        assert_eq!(
            shown.pointer("/result/subject"),
            Some(&json!({"about":subject,"title":{"kind":"shown","title":title}})),
            "{shown}"
        );
        if variant == "workspaceAdr" {
            let adr: AdrId =
                serde_json::from_value(subject.get("subject").ok_or("ADR input")?.clone())?;
            let mut cache = sqlx_turso::TursoConnectOptions::new()
                .filename(fixture._directory.path().join("fleet-cache.sqlite"))
                .connect()
                .await?;
            let removed = sqlx::query("DELETE FROM cached_fleet_records WHERE kind='adr' AND id=?")
                .bind(adr.as_str())
                .execute(&mut cache)
                .await?;
            assert_eq!(
                removed.rows_affected(),
                1,
                "fixture must remove the actual cached ADR only"
            );
            let unavailable = fixture
                .api
                .call(
                    "thread_show",
                    json!({"thread":thread_of(&request),"reader":{"kind":"anonymous"}}),
                )
                .await?;
            assert_eq!(unavailable.pointer("/result/subject/about"), Some(&subject));
            assert_eq!(
                unavailable.pointer("/result/subject/title"),
                Some(&json!({"kind":"unavailable"})),
                "{unavailable}"
            );
            cache.close().await?;
        }
        if variant == "workstream" {
            accepted(&fixture.api.call("workstream_rename", fixture.request(&project, json!({"kind":"workstreamRename","workstreamId":workstream,"name":"Current name","expectedRevision":0}))).await?);
            let shown = fixture
                .api
                .call(
                    "thread_show",
                    json!({"thread":thread_of(&request),"reader":{"kind":"anonymous"}}),
                )
                .await?;
            assert_eq!(
                shown.pointer("/result/subject/title/title"),
                Some(&json!("Current name"))
            );
        }
    }
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; Result propagates fixture errors."
)]
async fn scope_checks_reject_foreign_records_and_cache_not_found_replays_after_arrival()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let (foreign, foreign_stream) = fixture.project().await?;
    let foreign_task = fixture.task(&foreign, &foreign_stream).await?;
    let row = WorkTrailRowId::generate();
    accepted(&fixture.api.call("work_trail_add", fixture.request(&foreign, json!({"kind":"workTrailAdd","rowId":row,"subject":{"kind":"project"},"text":"foreign row","tags":[],"links":[]}))).await?);
    for (kind, id) in [
        ("workstream", json!(foreign_stream)),
        ("task", json!(foreign_task)),
        ("workTrailRow", json!(row)),
    ] {
        let refusal = fixture
            .api
            .call(
                "thread_create",
                create(&fixture, &project, Some(json!({"kind":kind,"subject":id})))?,
            )
            .await?;
        assert_eq!(refusal_kind(&refusal), Some("notFound"), "{refusal}");
        assert_eq!(
            refusal.pointer("/result/result/decision/reasons/0/reason/id"),
            Some(&id)
        );
    }
    let workspace = WorkspaceId::generate();
    let mut create_workspace = fixture.header();
    create_workspace
        .as_object_mut()
        .ok_or("workspace header")?
        .extend(
            json!({"workspaceId":workspace,"name":"other workspace","description":""})
                .as_object()
                .ok_or("workspace fields")?
                .clone(),
        );
    accepted(
        &fixture
            .api
            .call("workspace_create", create_workspace)
            .await?,
    );
    let create_adr = |workspace: &WorkspaceId, adr: &AdrId| {
        let mut request = fixture.header();
        request.as_object_mut().expect("header").extend(json!({"workspaceId":workspace,"adrId":adr,"title":"ADR title","text":"ADR content","supersedes":null,"madeIn":null,"repositoryFiles":[]}).as_object().expect("adr").clone());
        request
    };
    let foreign_adr = AdrId::generate();
    accepted(
        &fixture
            .api
            .call("adr_create", create_adr(&workspace, &foreign_adr))
            .await?,
    );
    let refusal = fixture
        .api
        .call(
            "thread_create",
            create(
                &fixture,
                &project,
                Some(json!({"kind":"workspaceAdr","subject":foreign_adr})),
            )?,
        )
        .await?;
    assert_eq!(refusal_kind(&refusal), Some("invalidShape"), "{refusal}");
    assert_eq!(
        refusal.pointer("/result/result/decision/reasons/0/reason/field"),
        Some(&json!("about"))
    );
    let late = AdrId::generate();
    let mut request = create(
        &fixture,
        &project,
        Some(json!({"kind":"workspaceAdr","subject":late})),
    )?;
    let missing = fixture.api.call("thread_create", request.clone()).await?;
    assert_eq!(refusal_kind(&missing), Some("notFound"), "{missing}");
    accepted(
        &fixture
            .api
            .call("adr_create", create_adr(&fixture.workspace, &late))
            .await?,
    );
    let replay = fixture.api.call("thread_create", request.clone()).await?;
    assert_eq!(
        replay.pointer("/result/result"),
        missing.pointer("/result/result")
    );
    request["requestId"] = json!(RequestId::generate());
    accepted(&fixture.api.call("thread_create", request).await?);
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; Result propagates fixture errors."
)]
async fn row_subject_titles_follow_redaction_without_copying_private_text_into_thread()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let row = WorkTrailRowId::generate();
    accepted(&fixture.api.call("work_trail_add", fixture.request(&project, json!({"kind":"workTrailAdd","rowId":row,"subject":{"kind":"project"},"text":"Private first line\nOther line","tags":[],"links":[]}))).await?);
    let request = create(
        &fixture,
        &project,
        Some(json!({"kind":"workTrailRow","subject":row})),
    )?;
    accepted(&fixture.api.call("thread_create", request.clone()).await?);
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
    let show = |include| json!({"thread":thread_of(&request),"reader":{"kind":"anonymous"},"includeRedacted":include});
    let hidden = fixture.api.call("thread_show", show(false)).await?;
    assert_eq!(
        hidden.pointer("/result/subject/title"),
        Some(&json!({"kind":"redacted"}))
    );
    assert!(!hidden.to_string().contains("Private first line"));
    let explicit = fixture.api.call("thread_show", show(true)).await?;
    assert_eq!(
        explicit.pointer("/result/subject/title/title"),
        Some(&json!("Private first line"))
    );
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; Result propagates fixture errors."
)]
async fn missing_and_invalid_subjects_and_project_target_posts_are_refused() -> Result<(), TestError>
{
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    for about in [
        None,
        Some(Value::Null),
        Some(project_subject("")),
        Some(project_subject("two\nlines")),
        Some(project_subject(&"é".repeat(121))),
    ] {
        let result = fixture
            .api
            .call("thread_create", create(&fixture, &project, about)?)
            .await?;
        assert_eq!(
            result.pointer("/error/data/kind"),
            Some(&json!("invalidShape")),
            "{result}"
        );
        assert_eq!(
            result.pointer("/error/data/details/field"),
            Some(&json!("about")),
            "{result}"
        );
    }
    let mut legacy = fixture.header();
    legacy.as_object_mut().ok_or("header")?.extend(json!({"postId":PostId::generate(),"placement":{"kind":"project","projectId":project},"text":"no implicit root","references":[],"addressees":[]}).as_object().ok_or("post fields")?.clone());
    let refused = fixture.api.call("thread_post", legacy).await?;
    assert!(
        refused.get("error").is_some(),
        "project target created a subjectless root: {refused}"
    );
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; Result propagates fixture errors."
)]
async fn project_subject_and_refile_require_any_project_role_and_record_exact_history()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    for role in [
        "lead",
        "advisor",
        "implementer",
        "verifier",
        "reviewer",
        "operator",
    ] {
        let (project, workstream) = fixture.project().await?;
        let holder = if role == "lead" {
            fixture.actor.clone()
        } else {
            json!({"kind":"human","humanId":format!("holder-{role}")})
        };
        if role != "lead" {
            let subject = if role == "advisor" {
                json!({"kind":"project","subject":project})
            } else {
                json!({"kind":"workstream","subject":workstream})
            };
            let revision = u64::from(role == "advisor");
            accepted(&fixture.api.call("role_grant", fixture.request(&project, json!({"kind":"roleGrant","subject":subject,"role":role,"holder":holder,"model":null,"lineage":null,"expectedRevision":revision,"ownerGrant":null}))).await?);
        }
        let mut denied = create(&fixture, &project, Some(project_subject("outsider")))?;
        denied["actor"] = json!({"kind":"human","humanId":"no-role"});
        let result = fixture.api.call("thread_create", denied).await?;
        assert_eq!(refusal_kind(&result), Some("roleRequired"), "{result}");
        assert_eq!(
            result.pointer("/result/result/decision/reasons/0/reason/requirement"),
            Some(&json!({"kind":"anyProjectRole"}))
        );
        let mut request = create(&fixture, &project, Some(project_subject("Role discussion")))?;
        request["actor"] = holder.clone();
        accepted(&fixture.api.call("thread_create", request.clone()).await?);
        let about = json!({"kind":"workstream","subject":workstream});
        let mut refile = fixture.header();
        refile.as_object_mut().ok_or("header")?.extend(
            json!({"thread":thread_of(&request),"about":about,"actor":holder})
                .as_object()
                .ok_or("refile fields")?
                .clone(),
        );
        let mut outsider = refile.clone();
        outsider["requestId"] = json!(RequestId::generate());
        outsider["actor"] = json!({"kind":"human","humanId":"no-role"});
        let refused = fixture.api.call("thread_refile", outsider).await?;
        assert_eq!(refusal_kind(&refused), Some("roleRequired"), "{refused}");
        accepted(&fixture.api.call("thread_refile", refile.clone()).await?);
        let repeated = fixture.api.call("thread_refile", refile.clone()).await?;
        accepted(&repeated);
        let store = fixture
            ._registry
            .existing_project(&project)
            .await?
            .ok_or("store")?;
        let mut snapshot = store.snapshot().await?;
        let FeedRead::Entries { entries, .. } = snapshot
            .feed_after(
                FeedCursor {
                    epoch: EpochNumber::try_from(1)?,
                    position: FeedPosition::ZERO,
                },
                1000,
                true,
            )
            .await?
        else {
            return Err("feed line".into());
        };
        let records: Vec<_> = entries
            .into_iter()
            .filter_map(|entry| match entry.content {
                FeedContent::Shown(record) => Some(record),
                FeedContent::Redacted => None,
            })
            .collect();
        let events: Vec<_> = records
            .iter()
            .filter(|record| {
                serde_json::to_value(record)
                    .ok()
                    .is_some_and(|value| value.get("kind") == Some(&json!("threadRefiled")))
            })
            .collect();
        assert_eq!(events.len(), 1);
        let event = serde_json::to_value(events.first().ok_or("refile event")?)?;
        assert_eq!(
            event.pointer("/record/from"),
            Some(&project_subject("Role discussion"))
        );
        assert_eq!(event.pointer("/record/to"), Some(&about));
        snapshot.finish().await?;
        let shown = fixture
            .api
            .call(
                "thread_show",
                json!({"thread":thread_of(&request),"reader":{"kind":"anonymous"}}),
            )
            .await?;
        assert_eq!(shown.pointer("/result/about"), Some(&about));
        refile["requestId"] = json!(RequestId::generate());
        let no_op = fixture.api.call("thread_refile", refile).await?;
        assert_eq!(
            no_op.pointer("/result/result/decision/change"),
            Some(&json!("alreadySo"))
        );
    }
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; Result propagates fixture errors."
)]
async fn session_role_witness_and_implicit_trail_use_their_own_scoped_operands()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, stream) = fixture.project().await?;
    let actor = json!({"kind":"session","session":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"m2b-scoped-role-fixture"}});
    board_fixture::accepted(&fixture.api.call("role_grant", fixture.request(&project, json!({"kind":"roleGrant","subject":{"kind":"workstream","subject":stream},"role":"operator","holder":actor,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}))).await?);
    let mut root = create(
        &fixture,
        &project,
        Some(project_subject("Session role holder")),
    )?;
    root["actor"] = actor.clone();
    root["role"] = json!("orchestrator");
    accepted(&fixture.api.call("thread_create", root).await?);
    let trail_request = |row: WorkTrailRowId| {
        let mut request = fixture.request(&project, json!({"kind":"workTrailAdd","rowId":row,"subject":null,"text":"Session implicit subject","tags":[],"links":[]}));
        request["actor"] = actor.clone();
        request
    };
    let refused = fixture
        .api
        .call("work_trail_add", trail_request(WorkTrailRowId::generate()))
        .await?;
    assert_eq!(refusal_kind(&refused), Some("subjectRequired"), "{refused}");
    assert_eq!(
        refused.pointer("/result/result/decision/reasons/0/reason/assignments"),
        Some(&json!({
            "entries":[],"totalCount":0,"abridged":false,
            "digest":"4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945"
        }))
    );
    accepted(&fixture.api.call("role_grant", fixture.request(&project, json!({"kind":"roleGrant","subject":{"kind":"workstream","subject":stream},"role":"implementer","holder":actor,"model":null,"lineage":null,"expectedRevision":1,"ownerGrant":null}))).await?);
    let row = WorkTrailRowId::generate();
    accepted(
        &fixture
            .api
            .call("work_trail_add", trail_request(row.clone()))
            .await?,
    );
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    let Some(Record::WorkTrailRow(row)) = read.record(&row.into()).await? else {
        return Err("trail row".into());
    };
    assert_eq!(row.subject, TrailSubject::Workstream(stream));
    read.finish().await?;
    fixture.stop().await
}
