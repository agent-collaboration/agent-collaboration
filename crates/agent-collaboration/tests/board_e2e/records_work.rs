//! BR08 / VD §5a U4-group,U9,U10,U41,U42,U47,U79: durable keyed effects and causal facts.
use super::{
    fixture::{BoardFixture, accepted, at, decision, fields, header, identity},
    record_inputs::{definition, evaluator},
};
use codex_router_test_support::FixtureResult;
use serde_json::{Value, json};

fn reasons(reply: &Value) -> FixtureResult<Vec<Value>> {
    at(decision(reply)?, "/reasons")?
        .as_array()
        .ok_or("refusal reasons absent")?
        .iter()
        .map(|reason| {
            if reason.get("kind") == Some(&json!("move")) {
                at(reason, "/reason/kind").cloned()
            } else {
                at(reason, "/kind").cloned()
            }
        })
        .collect()
}

pub async fn replay_moves_and_late_facts(fixture: &mut BoardFixture) -> FixtureResult {
    // Wrong builds: retry repeats an effect, changed content overwrites a request, or late facts rewrite history.
    fixture.create().await?;
    keyed_post_replay(fixture).await?;
    let check = refuse_unproven_move(fixture).await?;
    accept_proven_move_and_late_fact(fixture, check).await?;
    super::records_reopen::assert_reopen(fixture).await
}

async fn keyed_post_replay(fixture: &BoardFixture) -> FixtureResult {
    let root = identity();
    let text = fixture.root.path().join("recorded-root.txt");
    std::fs::write(&text, "recorded root")?;
    accepted(at(
        &fixture
            .cli(&[
                "thread",
                "create",
                "--project-id",
                &fixture.project,
                "--post-id",
                &root,
                "--about",
                "project:recorded-work",
                "--actor",
                &fixture.owner.to_string(),
                "--no-watch",
                "--text-file",
                text.to_str().ok_or("root path")?,
            ])
            .await?,
        "/record",
    )?)?;
    let post = identity();
    let request = fields(
        header(&fixture.owner),
        json!({"postId":post,
        "placement":{"kind":"thread","thread":{"project":fixture.project,"thread":root}},
        "text":"original keyed immutable post","references":[],"addressees":[]}),
    )?;
    let first = fixture.accept("thread_post", request.clone()).await?;
    let original = fixture
        .host()?
        .mcp("post_show", json!({"postId":post}))
        .await?;
    let before = fixture.feed().await?;
    let replay = fixture.write("thread_post", request.clone()).await?;
    assert_eq!(accepted(&first)?, accepted(&replay)?);
    assert_eq!(
        at(&fixture.feed().await?, "/value")?,
        at(&before, "/value")?
    );
    let conflict = fixture
        .write(
            "thread_post",
            fields(request, json!({"text":"conflicting content"}))?,
        )
        .await?;
    assert_eq!(
        conflict.pointer("/result/kind"),
        Some(&json!("conflictingRequest"))
    );
    assert_eq!(
        fixture
            .host()?
            .mcp("post_show", json!({"postId":post}))
            .await?,
        original
    );
    assert_eq!(
        at(&fixture.feed().await?, "/value")?,
        at(&before, "/value")?
    );
    Ok(())
}

fn move_request(fixture: &BoardFixture, reason: &str) -> Value {
    fixture.request(&fixture.owner,json!({"kind":"moveRequest",
        "moveId":identity(),"moveName":"finish","evidence":[],"reason":reason,"expectedRevision":1,"addressees":[]}))
}

async fn refuse_unproven_move(fixture: &BoardFixture) -> FixtureResult<Value> {
    definition(fixture, true).await?;
    let check = json!({"projectId":fixture.project,"actor":fixture.owner,"moveName":"finish","expectedRevision":1});
    let before_check = fixture.feed().await?;
    let verdict = fixture.host()?.mcp("move_check", check.clone()).await?;
    assert_eq!(verdict.get("kind"), Some(&json!("rejected")));
    let refusal_kinds = at(&verdict, "/reasons")?
        .as_array()
        .ok_or("move-check reasons")?
        .iter()
        .map(|reason| at(reason, "/kind").cloned())
        .collect::<FixtureResult<Vec<_>>>()?;
    assert_eq!(refusal_kinds, vec![json!("guardUnavailable")]);
    assert_eq!(
        at(&fixture.feed().await?, "/value")?,
        at(&before_check, "/value")?
    );
    let refused_request = move_request(fixture, "attempt without recorded proof");
    let refused = fixture
        .write("move_request", refused_request.clone())
        .await?;
    assert_eq!(reasons(&refused)?, vec![json!("guardUnavailable")]);
    let refused_head = fixture.feed().await?;
    let refused_replay = fixture.write("move_request", refused_request).await?;
    assert_eq!(refused.get("result"), refused_replay.get("result"));
    assert_eq!(
        at(&fixture.feed().await?, "/value")?,
        at(&refused_head, "/value")?
    );
    Ok(check)
}

async fn accept_proven_move_and_late_fact(fixture: &BoardFixture, check: Value) -> FixtureResult {
    fixture
        .accept("fact_record", evaluator(fixture, Value::Null))
        .await?;
    let before_allowed = fixture.feed().await?;
    let allowed = fixture.host()?.mcp("hook_check", check).await?;
    assert_eq!(allowed.get("kind"), Some(&json!("ok")));
    assert_eq!(allowed.pointer("/payload/kind"), Some(&json!("check")));
    assert_eq!(
        allowed.pointer("/payload/payload/kind"),
        Some(&json!("allowed"))
    );
    assert_eq!(
        at(&fixture.feed().await?, "/value")?,
        at(&before_allowed, "/value")?
    );
    let accepted_request = move_request(fixture, "recorded proof now met");
    let moved = fixture
        .accept("move_request", accepted_request.clone())
        .await?;
    let moved_head = fixture.feed().await?;
    assert_eq!(
        accepted(&fixture.write("move_request", accepted_request).await?)?,
        accepted(&moved)?
    );
    assert_eq!(
        at(&fixture.feed().await?, "/value")?,
        at(&moved_head, "/value")?
    );
    let projects = fixture.feed_records("project").await?;
    assert_eq!(
        projects.last().and_then(|project| project.get("status")),
        Some(&json!("done"))
    );
    let before_close = at(&fixture.feed().await?, "/value/highWater")?.clone();
    fixture
        .accept(
            "project_close",
            fixture.request(
                &fixture.owner,
                json!({"kind":"projectClose",
        "reason":"verification complete","expectedRevision":2}),
            ),
        )
        .await?;
    let late = evaluator(fixture, before_close.clone());
    let late_id = at(&late, "/operation/fact/factId")?.clone();
    fixture.accept("fact_record", late).await?;
    let fact = fixture
        .host()?
        .mcp(
            "fact_show",
            json!({"projectId":fixture.project,"record":{"kind":"fact","id":late_id}}),
        )
        .await?;
    assert_eq!(fact.pointer("/record/late"), Some(&json!(true)));
    assert_eq!(fact.pointer("/record/anchor"), Some(&before_close));
    assert_eq!(
        reasons(
            &fixture
                .write("fact_record", evaluator(fixture, Value::Null))
                .await?
        )?,
        vec![json!("closed")]
    );
    let intents = fixture.feed_records("notifyIntent").await?;
    assert!(
        !intents
            .iter()
            .any(|intent| intent.pointer("/cause/id") == Some(&late_id))
    );
    Ok(())
}
