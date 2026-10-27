//! BC04: forwarding keeps identity/content and binds origin to enrollment.
use super::{
    central_pair::{SecondHost, await_view, settled_view},
    central_replication::matching_posts,
    fixture::{BoardFixture, accepted, at, fields, identity},
};
use codex_router_test_support::FixtureResult;
use serde_json::{Value, json};

pub async fn verified_origin(fixture: &mut BoardFixture, sender: &mut SecondHost) -> FixtureResult {
    fixture.create().await?;
    let thread = fixture.thread(&fixture.project, &fixture.owner).await?;
    let view = settled_view(fixture.host()?, &fixture.project).await?;
    await_view(sender.host()?, &fixture.project, at(&view, "/asOf")?).await?;
    let request =
        fixture.reply_request(&fixture.project, &thread, "BC04 actual forwarded content")?;
    let forged = fields(
        request.clone(),
        json!({
        "requestId":identity(),"postId":identity(),
        "origin":{"installation":identity(),"machine":"spoofed sender"}}),
    )?;
    let refusal = sender.host()?.mcp_reply("thread_post", forged).await?;
    assert!(
        refusal.is_error,
        "sender-supplied origin must not become a trusted origin"
    );
    assert_eq!(
        settled_view(fixture.host()?, &fixture.project)
            .await?
            .get("asOf"),
        view.get("asOf")
    );
    let answer = sender.host()?.mcp("thread_post", request.clone()).await?;
    accepted(&answer)?;
    let after = settled_view(fixture.host()?, &fixture.project).await?;
    assert_eq!(
        sender
            .host()?
            .mcp("thread_post", request.clone())
            .await?
            .get("result"),
        answer.get("result")
    );
    assert_eq!(
        settled_view(fixture.host()?, &fixture.project)
            .await?
            .get("asOf"),
        after.get("asOf")
    );
    assert_forwarded(fixture, sender, &request).await
}

pub async fn assert_forwarded(
    fixture: &BoardFixture,
    sender: &SecondHost,
    request: &Value,
) -> FixtureResult {
    let feed = fixture.feed().await?;
    assert_forwarded_entry(&feed, sender, request)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
pub fn assert_forwarded_entry(feed: &Value, sender: &SecondHost, request: &Value) -> FixtureResult {
    let posts = matching_posts(feed, at(request, "/postId")?)?;
    assert_eq!(
        posts.len(),
        1,
        "forwarded request creates exactly one holder record"
    );
    let post = posts.first().ok_or("forwarded post absent")?;
    assert_eq!(
        post.pointer("/origin/installation"),
        Some(&json!(sender.service_id()?))
    );
    assert_eq!(post.get("actor"), request.get("actor"));
    assert_eq!(
        post.pointer("/content/record/record/text"),
        request.get("text")
    );
    assert_eq!(post.get("request"), request.get("requestId"));
    Ok(())
}
