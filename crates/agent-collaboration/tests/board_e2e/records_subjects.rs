//! BR11 / VD §5a U4-group,U26–31,U79: subject grammar, authority and recorded refile.
use super::{
    fixture::{BoardFixture, accepted, at, decision, fields, header, human, identity},
    role_checks::{Roles, rejected, role_required},
};
use codex_router_test_support::FixtureResult;
use serde_json::{Value, json};
use std::ffi::OsStr;

fn request(fixture: &BoardFixture, actor: &Value, about: Value) -> FixtureResult<Value> {
    fields(
        header(actor),
        json!({"projectId":fixture.project,"postId":identity(),"about":about,
        "text":"subject discussion","references":[],"addressees":[],"role":null,"watch":false}),
    )
}

async fn create(
    fixture: &BoardFixture,
    actor: &Value,
    about: Value,
    grammar: &str,
) -> FixtureResult<String> {
    let request = request(fixture, actor, about)?;
    let post = at(&request, "/postId")?
        .as_str()
        .ok_or("post ID missing")?
        .to_owned();
    let request_id = at(&request, "/requestId")?
        .as_str()
        .ok_or("request ID missing")?;
    let text = fixture.root.path().join(format!("subject-{post}.txt"));
    std::fs::write(&text, "subject discussion")?;
    let actor = actor.to_string();
    let cli = fixture
        .cli(&[
            "thread",
            "create",
            "--project-id",
            &fixture.project,
            "--post-id",
            &post,
            "--request-id",
            request_id,
            "--about",
            grammar,
            "--actor",
            &actor,
            "--no-watch",
            "--text-file",
            text.to_str().ok_or("text path")?,
        ])
        .await?;
    let mcp = fixture.host()?.mcp("thread_create", request).await?;
    assert_eq!(at(&cli, "/record/result")?, at(&mcp, "/result")?);
    accepted(&mcp)?;
    Ok(post)
}

async fn show(fixture: &BoardFixture, thread: &str) -> FixtureResult<Value> {
    let mcp = fixture
        .host()?
        .mcp(
            "thread_show",
            json!({"thread":{"project":fixture.project,"thread":thread},
        "reader":{"kind":"anonymous"}}),
        )
        .await?;
    let cli = fixture
        .cli(&[
            "thread",
            "show",
            "--thread-id",
            &format!("{}/{thread}", fixture.project),
        ])
        .await?;
    assert_eq!(at(&cli, "/record")?, &mcp);
    Ok(mcp)
}

async fn expect_absent(fixture: &BoardFixture, post: &str) -> FixtureResult {
    let absent = fixture
        .host()?
        .mcp_reply("post_show", json!({"postId":post}))
        .await?;
    assert!(absent.is_error);
    assert_eq!(absent.value.get("kind"), Some(&json!("resourceNotFound")));
    Ok(())
}

struct SubjectCase {
    advisor: Value,
    outsider: Value,
    initial: Value,
    thread: String,
    target: Value,
}
struct RefiledCase {
    request: Value,
    reply: Value,
    events: Vec<Value>,
}

pub async fn subjects_and_refiling(fixture: &mut BoardFixture) -> FixtureResult {
    // Keep explicit --about grammar calls, refusal atomicity and current-title/replay observations.
    fixture.create().await?;
    let case = prepare_subject(fixture).await?;
    refuse_subjectless_and_unauthorized_roots(fixture, &case).await?;
    observe_record_subjects(fixture).await?;
    refuse_missing_subject(fixture, &case).await?;
    let refiled = refile_and_rename(fixture, &case).await?;
    observe_refile_replay_and_authority(fixture, &case, refiled).await?;
    super::records_reopen::assert_reopen(fixture).await
}

async fn prepare_subject(fixture: &BoardFixture) -> FixtureResult<SubjectCase> {
    let mut roles = Roles::new();
    let advisor = human("subject-advisor");
    let outsider = human("subject-outsider");
    roles
        .grant(
            fixture,
            &fixture.owner,
            "project",
            "advisor",
            &advisor,
            json!("changed"),
        )
        .await?;
    let initial = json!({"kind":"project","subject":{"line":"named project decision"}});
    let thread = create(
        fixture,
        &advisor,
        initial.clone(),
        "project:named project decision",
    )
    .await?;
    let shown = show(fixture, &thread).await?;
    assert_eq!(shown.get("about"), Some(&initial));
    assert_eq!(shown.get("imported"), Some(&json!(false)));
    assert_eq!(
        shown.get("subject"),
        Some(&json!({"about":initial,"title":{"kind":"shown","title":"named project decision"}}))
    );
    let target = json!({"kind":"workstream","subject":fixture.workstream});
    Ok(SubjectCase {
        advisor,
        outsider,
        initial,
        thread,
        target,
    })
}

async fn refuse_subjectless_and_unauthorized_roots(
    fixture: &BoardFixture,
    case: &SubjectCase,
) -> FixtureResult {
    let SubjectCase {
        advisor,
        outsider,
        initial,
        thread,
        target,
    } = case;
    let text = fixture.root.path().join("refused-root.txt");
    std::fs::write(&text, "forbidden root")?;
    for removed in [false, true] {
        let post = identity();
        let actor = advisor.to_string();
        let mut args = if removed {
            vec!["post", "post", "--placement", "project"]
        } else {
            vec!["thread", "create", "--no-watch"]
        };
        args.extend([
            "--project-id",
            &fixture.project,
            "--post-id",
            &post,
            "--actor",
            &actor,
            "--text-file",
            text.to_str().ok_or("text path")?,
        ]);
        let arguments = args.iter().map(OsStr::new).collect::<Vec<_>>();
        let output = fixture.host()?.cli_output(&arguments).await?;
        assert_eq!(output.status.code(), Some(2));
        expect_absent(fixture, &post).await?;
    }
    fixture
        .accept(
            "thread_post",
            fields(
                header(&fixture.owner),
                json!({"postId":identity(),
        "placement":{"kind":"thread","thread":{"project":fixture.project,"thread":thread}},
        "text":"existing thread control","references":[],"addressees":[]}),
            )?,
        )
        .await?;
    let removed = identity();
    let refused = fixture
        .host()?
        .mcp_reply(
            "thread_post",
            fields(
                header(advisor),
                json!({
        "postId":removed,"placement":{"kind":"project","projectId":fixture.project},
        "text":"removed root entry","references":[],"addressees":[]}),
            )?,
        )
        .await?;
    assert!(refused.is_error);
    assert_eq!(refused.value.get("stage"), Some(&json!("validation")));
    expect_absent(fixture, &removed).await?;
    let refused = fixture
        .write(
            "thread_create",
            request(fixture, outsider, initial.clone())?,
        )
        .await?;
    rejected(
        &refused,
        json!([role_required(json!({"kind":"anyProjectRole"}))]),
    )?;
    let open = create(
        fixture,
        outsider,
        target.clone(),
        &format!("workstream:{}", fixture.workstream),
    )
    .await?;
    assert_eq!(show(fixture, &open).await?.get("about"), Some(target));
    Ok(())
}

async fn observe_record_subjects(fixture: &BoardFixture) -> FixtureResult {
    let task = super::record_inputs::task(fixture, "task subject title").await?;
    let row = identity();
    let adr = identity();
    fixture.accept("work_trail_add",fixture.request(&fixture.owner,json!({"kind":"workTrailAdd",
        "rowId":row,"subject":{"kind":"project"},"text":"row first line\nrow detail","tags":[],"links":[]}))).await?;
    fixture
        .accept(
            "adr_create",
            fields(
                header(&fixture.owner),
                json!({"workspaceId":fixture.workspace,
        "adrId":adr,"title":"workspace decision","text":"decision content","supersedes":null,
        "madeIn":null,"repositoryFiles":[]}),
            )?,
        )
        .await?;
    for (grammar, kind, record, title) in [
        ("task", "task", task, "task subject title"),
        ("row", "workTrailRow", row, "row first line"),
        ("adr", "workspaceAdr", adr, "workspace decision"),
    ] {
        let about = json!({"kind":kind,"subject":record});
        let post = create(
            fixture,
            &human(&format!("subject-{grammar}-outsider")),
            about.clone(),
            &format!("{grammar}:{record}"),
        )
        .await?;
        let shown = show(fixture, &post).await?;
        assert_eq!(shown.get("about"), Some(&about));
        assert_eq!(shown.get("imported"), Some(&json!(false)));
        assert_eq!(
            shown.get("subject"),
            Some(&json!({"about":about,"title":{"kind":"shown","title":title}}))
        );
    }
    Ok(())
}

async fn refuse_missing_subject(fixture: &BoardFixture, case: &SubjectCase) -> FixtureResult {
    let SubjectCase {
        outsider, initial, ..
    } = case;
    let mut missing = request(fixture, outsider, initial.clone())?;
    missing
        .as_object_mut()
        .ok_or("request object")?
        .remove("about");
    let refusal = fixture.host()?.mcp_reply("thread_create", missing).await?;
    assert!(refusal.is_error);
    assert_eq!(refusal.value.get("kind"), Some(&json!("invalidShape")));
    assert_eq!(
        refusal.value.pointer("/details/field"),
        Some(&json!("about"))
    );
    Ok(())
}

async fn refile_and_rename(
    fixture: &BoardFixture,
    case: &SubjectCase,
) -> FixtureResult<RefiledCase> {
    let SubjectCase {
        advisor,
        initial,
        thread,
        target,
        ..
    } = case;
    let refile = fields(
        header(advisor),
        json!({"thread":{"project":fixture.project,"thread":thread},"about":target}),
    )?;
    let before = fixture.feed_records("threadRefiled").await?;
    let request_id = at(&refile, "/requestId")?
        .as_str()
        .ok_or("refile request ID")?;
    let cli = fixture
        .cli(&[
            "thread",
            "refile",
            "--thread-id",
            &format!("{}/{thread}", fixture.project),
            "--request-id",
            request_id,
            "--actor",
            &advisor.to_string(),
            "--about",
            &format!("workstream:{}", fixture.workstream),
        ])
        .await?;
    let reply = fixture.write("thread_refile", refile.clone()).await?;
    assert_eq!(at(&cli, "/record/result")?, at(&reply, "/result")?);
    assert_eq!(accepted(&reply)?.get("change"), Some(&json!("changed")));
    let events = fixture.feed_records("threadRefiled").await?;
    assert_eq!(events.len(), before.len() + 1);
    let event = events.first().ok_or("refile event absent")?;
    assert_eq!(event.get("thread"), Some(&json!(thread)));
    assert_eq!(event.get("from"), Some(initial));
    assert_eq!(event.get("to"), Some(target));
    assert_eq!(
        show(fixture, thread).await?.get("subject"),
        Some(&json!({"about":target,"title":{"kind":"shown","title":"verification lane"}}))
    );
    fixture
        .accept(
            "workstream_rename",
            fixture.request(
                &fixture.owner,
                json!({"kind":"workstreamRename",
        "workstreamId":fixture.workstream,"name":"Renamed verification lane","expectedRevision":0}),
            ),
        )
        .await?;
    let current = show(fixture, thread).await?;
    assert_eq!(
        current.get("subject"),
        Some(&json!({"about":target,"title":{"kind":"shown","title":"Renamed verification lane"}}))
    );
    Ok(RefiledCase {
        request: refile,
        reply,
        events,
    })
}

async fn observe_refile_replay_and_authority(
    fixture: &BoardFixture,
    case: &SubjectCase,
    refiled: RefiledCase,
) -> FixtureResult {
    let SubjectCase {
        outsider,
        initial,
        thread,
        target,
        ..
    } = case;
    let RefiledCase {
        request: refile,
        reply,
        events,
    } = refiled;
    let replay = fixture.write("thread_refile", refile.clone()).await?;
    assert_eq!(decision(&reply)?, decision(&replay)?);
    assert_eq!(fixture.feed_records("threadRefiled").await?, events);
    let no_op = fixture
        .accept(
            "thread_refile",
            fields(refile.clone(), json!({"requestId":identity()}))?,
        )
        .await?;
    assert_eq!(accepted(&no_op)?.get("change"), Some(&json!("alreadySo")));
    let denied = fixture
        .write(
            "thread_refile",
            fields(refile, fields(header(outsider), json!({"about":initial}))?)?,
        )
        .await?;
    rejected(
        &denied,
        json!([role_required(json!({"kind":"anyProjectRole"}))]),
    )?;
    assert_eq!(show(fixture, thread).await?.get("about"), Some(target));
    assert_eq!(fixture.feed_records("threadRefiled").await?, events);
    Ok(())
}
