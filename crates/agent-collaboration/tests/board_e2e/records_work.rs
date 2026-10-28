//! BR08 / VD §5a U4-group,U9,U10,U41,U42,U47,U79: durable keyed effects and causal facts.
use super::{
    fixture::{BoardFixture, accepted, at, decision, fields, header, identity},
    record_inputs::evaluator,
};
use codex_router_test_support::FixtureResult;
use serde_json::{Value, json};

fn reasons(reply: &Value) -> FixtureResult<Vec<Value>> {
    at(decision(reply)?, "/reasons")?
        .as_array()
        .ok_or("refusal reasons absent")?
        .iter()
        .map(|reason| at(reason, "/kind").cloned())
        .collect()
}

pub async fn replay_keyed_work_and_late_facts(fixture: &mut BoardFixture) -> FixtureResult {
    // Wrong builds: retry repeats an effect, changed content overwrites a request, or late facts rewrite history.
    fixture.create().await?;
    keyed_post_replay(fixture).await?;
    record_and_close_with_late_fact(fixture).await?;
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

async fn record_and_close_with_late_fact(fixture: &BoardFixture) -> FixtureResult {
    fixture
        .accept("fact_record", evaluator(fixture, Value::Null))
        .await?;
    let before_close = at(&fixture.feed().await?, "/value/highWater")?.clone();
    fixture
        .accept(
            "project_close",
            fixture.request(
                &fixture.owner,
                json!({"kind":"projectClose",
        "reason":"verification complete","expectedRevision":0}),
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
