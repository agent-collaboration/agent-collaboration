//! BR13 / G5: real poll settlement and policy continuity across an owning Host restart.
use super::fixture::{BoardFixture, accepted, at, fields, header, human};
use codex_router_test_support::FixtureResult;
use serde_json::{Value, json};

async fn membership(fixture: &BoardFixture, reader: &Value) -> FixtureResult<Value> {
    let records = fixture.feed_records("subscriptionMembership").await?;
    records
        .into_iter()
        .rfind(|record| record.get("reader") == Some(reader))
        .ok_or_else(|| "public feed omitted the subscribed reader".into())
}

async fn wait(fixture: &BoardFixture, reader: &Value, seconds: u64) -> FixtureResult<Value> {
    fixture
        .host()?
        .mcp(
            "thread_wait",
            json!({"protocolVersion":1,"actingFor":null,"actor":reader,"renewals":[],
                "filter":{"kind":"project","projectId":fixture.project},"maxWaitSeconds":seconds}),
        )
        .await
}

async fn post_range(fixture: &BoardFixture, thread: &str, post: &Value) -> FixtureResult<Value> {
    let shown = fixture
        .host()?
        .mcp("post_show", json!({"postId":at(post,"/postId")?}))
        .await?;
    let placement: message_board::RecordPlacement =
        serde_json::from_value(at(&shown, "/position")?.clone())?;
    let cursor = placement
        .project_cursor()
        .ok_or("post has no project cursor")?;
    Ok(json!({"threadId":thread,"projectId":fixture.project,
        "from":cursor,"through":cursor,"postCount":1}))
}

pub(super) async fn consume_once(
    fixture: &BoardFixture,
    reader: &Value,
    range: &Value,
) -> FixtureResult {
    let reply = wait(fixture, reader, 1).await?;
    assert_eq!(reply.pointer("/outcome/kind"), Some(&json!("batch")));
    assert_eq!(reply.pointer("/outcome/batch/kind"), Some(&json!("ranges")));
    assert_eq!(reply.pointer("/outcome/batch/roots"), Some(&json!([range])));
    let settled = wait(fixture, reader, 0).await?;
    assert_eq!(settled.pointer("/outcome/kind"), Some(&json!("timedOut")));
    Ok(())
}

async fn posts(fixture: &BoardFixture, thread: &str) -> FixtureResult<Value> {
    fixture
        .host()?
        .mcp(
            "post_list",
            json!({"scope":{"kind":"thread","thread":{"project":fixture.project,"thread":thread}},
                "selection":{"kind":"latest"},"page":{"limit":100}}),
        )
        .await
}

async fn show_thread(fixture: &BoardFixture, thread: &str) -> FixtureResult<Value> {
    fixture
        .host()?
        .mcp(
            "thread_show",
            json!({"thread":{"project":fixture.project,"thread":thread},
            "reader":{"kind":"anonymous"}}),
        )
        .await
}

pub async fn poll_restart(fixture: &mut BoardFixture) -> FixtureResult {
    // Wrong builds: a settled range repeats, policy replaces activation, or restart loses an owed range.
    fixture.create().await?;
    let thread = fixture.thread(&fixture.project, &fixture.owner).await?;
    let reader = human("poll-reader");
    fixture
        .accept(
            "thread_join",
            fields(
                json!({"requestId":super::fixture::identity(),"protocolVersion":1,"actor":&reader}),
                json!({"thread":{"project":fixture.project,"thread":thread},
                "role":"participant","watch":true,"replace":null,"note":null}),
            )?,
        )
        .await?;
    let subscribed = fixture
        .cli(&[
            "thread",
            "subscribe",
            "--thread-id",
            &format!("{}/{}", fixture.project, thread),
            "--actor",
            &reader.to_string(),
            "--mode",
            "poll",
            "--quiet",
            "0s",
            "--cap",
            "0s",
            "--for",
            "24h",
        ])
        .await?;
    accepted(at(&subscribed, "/record")?)?;
    let initial = membership(fixture, &reader).await?;
    assert_eq!(initial.get("reader"), Some(&reader));
    assert_eq!(initial.pointer("/policy/mode"), Some(&json!("poll")));
    assert_eq!(
        initial.pointer("/policy/timing/quietSeconds"),
        Some(&json!(0))
    );
    assert_eq!(
        initial.pointer("/policy/timing/capSeconds"),
        Some(&json!(0))
    );
    for text in ["first actual range", "message between bounded waits"] {
        let post = fixture.reply(&fixture.project, &thread, text).await?;
        let range = post_range(fixture, &thread, &post).await?;
        consume_once(fixture, &reader, &range).await?;
    }
    let owed = fixture
        .reply(
            &fixture.project,
            &thread,
            "owed range survives policy and restart",
        )
        .await?;
    let owed_range = post_range(fixture, &thread, &owed).await?;
    fixture
        .accept(
            "thread_subscribe",
            fields(header(&reader), json!({"scope":{"kind":"thread","thread":{"project":fixture.project,"thread":thread}},
                "policy":{"mode":"poll","quietSeconds":0,"capSeconds":1,"lifetimeSeconds":172800}}))?,
        )
        .await?;
    assert_policy_and_reopen(fixture, &reader, &thread, &initial, &owed_range).await
}

async fn assert_policy_and_reopen(
    fixture: &mut BoardFixture,
    reader: &Value,
    thread: &str,
    initial: &Value,
    owed_range: &Value,
) -> FixtureResult {
    let changed = membership(fixture, reader).await?;
    assert_eq!(at(&changed, "/envelope/id")?, at(initial, "/envelope/id")?);
    assert_eq!(at(&changed, "/activatedAt")?, at(initial, "/activatedAt")?);
    let generation = at(initial, "/generation")?
        .as_u64()
        .ok_or("membership generation")?;
    assert_eq!(changed.get("generation"), Some(&json!(generation + 1)));
    let subscriptions = fixture
        .host()?
        .mcp("thread_subscriptions", json!({"actor":reader}))
        .await?;
    assert_eq!(subscriptions.get("unavailable"), Some(&json!([])));
    let cli = fixture
        .cli(&["thread", "subscriptions", "--actor", &reader.to_string()])
        .await?;
    assert_eq!(cli.get("record"), Some(&subscriptions));
    let pending = at(&subscriptions, "/subscriptions")?
        .as_array()
        .ok_or("subscriptions")?;
    assert_eq!(pending.len(), 1);
    assert_eq!(
        pending.first().and_then(|row| row.get("pendingCount")),
        Some(&json!(1))
    );
    let before_posts = posts(fixture, thread).await?;
    let before_thread = show_thread(fixture, thread).await?;
    fixture.stop_host().await?;
    fixture.reopen_host().await?;
    assert_eq!(membership(fixture, reader).await?, changed);
    assert_eq!(show_thread(fixture, thread).await?, before_thread);
    assert_eq!(
        posts(fixture, thread).await?.pointer("/page/records"),
        before_posts.pointer("/page/records")
    );
    consume_once(fixture, reader, owed_range).await?;
    Ok(())
}
