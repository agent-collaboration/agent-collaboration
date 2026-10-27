#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! Native inbox projects typed state changes and keeps reader/scope continuation boundaries.
use message_board::*;
use serde_json::{Value, json};
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
use board_fixture::{BoardFixture, TestError, accepted};
fn fetch(project: &ProjectId, reader: &Value, cursor: Option<&str>, limit: u32) -> Value {
    json!({"protocolVersion":1,"actingFor":null,"reader":reader,"scope":{"kind":"project","projectId":project},"readMode":"unread","initializations":[],"page":{"limit":limit,"cursor":cursor}})
}
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove the test contract; Result propagates fixture errors."
)]
async fn watched_thread_lifecycle_is_unread_and_continuations_are_reader_bound()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let bob = json!({"kind":"human","humanId":"bob"});
    let mut initialize = fetch(&project, &bob, None, 100);
    initialize["initializations"] =
        json!([{"projectId":project,"requestId":RequestId::generate()}]);
    let initialized = fixture.api.call("inbox_fetch", initialize).await?;
    assert_eq!(
        initialized.pointer("/result/page/initializations/0/outcome/reply/result/decision/kind"),
        Some(&json!("accepted")),
        "{initialized}"
    );
    let root = PostId::generate();
    let thread = ThreadId::from(root.clone());
    let mut create = fixture.header();
    create["postId"] = json!(root);
    create["projectId"] = json!(project);
    create["about"] = json!({"kind":"project","subject":{"line":"Root"}});
    create["role"] = json!("orchestrator");
    create["watch"] = json!(false);
    create["text"] = json!("Root");
    create["references"] = json!([]);
    create["addressees"] = json!([]);
    accepted(&fixture.api.call("thread_create", create).await?);
    let mut watch = fixture.header();
    watch["actor"] = bob.clone();
    watch["thread"] = json!({"thread":thread});
    accepted(&fixture.api.call("thread_watch", watch).await?);
    let mut resolve = fixture.header();
    resolve["thread"] = json!({"thread":thread});
    resolve["reason"] = json!("completed");
    accepted(&fixture.api.call("thread_resolve", resolve).await?);
    let mut unresolve = fixture.header();
    unresolve["thread"] = json!({"thread":thread});
    accepted(&fixture.api.call("thread_unresolve", unresolve).await?);
    let reply = PostId::generate();
    let mut post = fixture.header();
    post["postId"] = json!(reply);
    post["placement"] = json!({"kind":"thread","thread":{"thread":thread}});
    post["text"] = json!("Reply");
    post["references"] = json!([]);
    post["addressees"] = json!([]);
    accepted(&fixture.api.call("thread_post", post).await?);
    let mut collected = Vec::new();
    let mut next = None;
    let mut first_cursor = None;
    let mut high_water = None;
    loop {
        let response = fixture
            .api
            .call("inbox_fetch", fetch(&project, &bob, next.as_deref(), 1))
            .await?;
        let page = response
            .pointer("/result/page")
            .ok_or_else(|| format!("{response}"))?;
        if let Some(expected) = &high_water {
            assert_eq!(&page["highWater"], expected);
        } else {
            high_water = Some(page["highWater"].clone());
        }
        collected.extend(page["records"].as_array().ok_or("records")?.iter().cloned());
        next = page["nextCursor"].as_str().map(str::to_owned);
        if first_cursor.is_none() {
            first_cursor = next.clone();
        }
        if next.is_none() {
            break;
        }
    }
    let kinds: Vec<_> = collected
        .iter()
        .filter_map(|row| row["kind"].as_str())
        .collect();
    assert_eq!(
        kinds,
        vec![
            "postCreated",
            "threadStateChanged",
            "threadStateChanged",
            "postCreated"
        ],
        "{collected:?}"
    );
    assert_eq!(collected[1]["state"], json!("resolved"));
    assert_eq!(collected[2]["state"], json!("open"));
    for pair in collected.windows(2) {
        assert!(
            pair[0]["position"]["position"]["cursor"]["position"].as_u64()
                < pair[1]["position"]["position"]["cursor"]["position"].as_u64()
        );
    }
    let other = json!({"kind":"human","humanId":"other"});
    let invalid = fixture
        .api
        .call(
            "inbox_fetch",
            fetch(&project, &other, first_cursor.as_deref(), 1),
        )
        .await?;
    assert_eq!(
        invalid.pointer("/error/data/kind"),
        Some(&json!("invalidCursor")),
        "{invalid}"
    );
    // A later post is outside the first browse's frozen high-water.
    let later = PostId::generate();
    let mut post = fixture.header();
    post["postId"] = json!(later);
    post["placement"] = json!({"kind":"thread","thread":{"thread":thread}});
    post["text"] = json!("Later");
    post["references"] = json!([]);
    post["addressees"] = json!([]);
    accepted(&fixture.api.call("thread_post", post).await?);
    let frozen = fixture
        .api
        .call(
            "inbox_fetch",
            fetch(&project, &bob, first_cursor.as_deref(), 100),
        )
        .await?;
    assert_eq!(
        Some(&frozen["result"]["page"]["highWater"]),
        high_water.as_ref()
    );
    assert!(
        !frozen.to_string().contains(later.as_str()),
        "continuation included a later write"
    );
    let mut acknowledge = json!({"protocolVersion":1,"actor":bob,"actingFor":null});
    acknowledge["actor"] = bob.clone();
    acknowledge["entries"] = json!([{"requestId":RequestId::generate(),"scope":{"kind":"thread","thread":{"thread":thread}},"through":high_water.as_ref().ok_or("high water")?[project.as_str()]}]);
    let acknowledged = fixture.api.call("inbox_acknowledge", acknowledge).await?;
    assert_eq!(
        acknowledged.pointer("/result/outcomes/0/outcome/reply/result/decision/kind"),
        Some(&json!("accepted")),
        "{acknowledged}"
    );
    let unread = fixture
        .api
        .call("inbox_fetch", fetch(&project, &bob, None, 100))
        .await?;
    let remaining = unread["result"]["page"]["records"]
        .as_array()
        .ok_or("remaining")?;
    assert_eq!(
        remaining.len(),
        2,
        "thread acknowledgement should leave independent root and later reply"
    );
    assert!(
        remaining
            .iter()
            .any(|row| row.pointer("/post/postId") == Some(&json!(root)))
    );
    assert!(
        remaining
            .iter()
            .any(|row| row.pointer("/post/postId") == Some(&json!(later)))
    );
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove the test contract; Result propagates fixture errors."
)]
async fn a_read_continuation_does_not_depend_on_initialization_request_metadata()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let reader = json!({"kind":"human","humanId":"page-reader"});
    let mut initial = fetch(&project, &reader, None, 1);
    initial["initializations"] = json!([{"projectId":project,"requestId":RequestId::generate()}]);
    let result = fixture.api.call("inbox_fetch", initial.clone()).await?;
    assert!(result.pointer("/result/page/initializations/0").is_some());
    let mut expected = Vec::new();
    for index in 0..2 {
        let id = PostId::generate();
        let mut post = fixture.header();
        post["actor"] = json!({"kind":"human","humanId":format!("writer-{index}")});
        post["postId"] = json!(id);
        post["projectId"] = json!(project);
        post["about"] = json!(fixture.discussion_subject(&project).await?);
        post["role"] = json!(null);
        post["watch"] = json!(false);
        post["text"] = json!("page");
        post["references"] = json!([]);
        post["addressees"] = json!([]);
        accepted(&fixture.api.call("thread_create", post).await?);
        expected.push(id);
    }
    let first = fixture.api.call("inbox_fetch", initial).await?;
    let cursor = first
        .pointer("/result/page/nextCursor")
        .and_then(Value::as_str)
        .ok_or("continuation")?;
    assert_eq!(
        first.pointer("/result/page/records/0/post/postId"),
        Some(&json!(expected[0]))
    );
    let second = fixture
        .api
        .call("inbox_fetch", fetch(&project, &reader, Some(cursor), 1))
        .await?;
    assert_eq!(
        second.pointer("/result/page/records/0/post/postId"),
        Some(&json!(expected[1])),
        "{second}"
    );
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove contextual scope behavior; Result propagates fixture errors."
)]
async fn a_scoped_inbox_keeps_outside_watched_threads_but_post_reads_stay_strict()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (selected, _) = fixture.project().await?;
    let (outside, _) = fixture.project().await?;
    let reader = json!({"kind":"human","humanId":"context-reader"});
    let mut initial = fetch(&selected, &reader, None, 100);
    initial["initializations"] = json!([{"projectId":selected,"requestId":RequestId::generate()}]);
    fixture.api.call("inbox_fetch", initial).await?;
    let mut roots = Vec::new();
    for project in [&selected, &outside] {
        let post = PostId::generate();
        let thread = ThreadId::from(post.clone());
        let mut request = fixture.header();
        request["projectId"] = json!(project);
        request["postId"] = json!(post);
        request["about"] = json!({"kind":"project","subject":{"line":"Context root"}});
        request["text"] = json!("context root");
        request["references"] = json!([]);
        request["addressees"] = json!([]);
        request["role"] = json!("orchestrator");
        request["watch"] = json!(false);
        accepted(&fixture.api.call("thread_create", request).await?);
        roots.push((post, thread));
    }
    let mut watch = fixture.header();
    watch["actor"] = reader.clone();
    watch["thread"] = json!({"thread":roots[1].1});
    accepted(&fixture.api.call("thread_watch", watch).await?);
    let reply = PostId::generate();
    let mut post = fixture.header();
    post["postId"] = json!(reply);
    post["placement"] = json!({"kind":"thread","thread":{"thread":roots[1].1}});
    post["text"] = json!("outside watched reply");
    post["references"] = json!([]);
    post["addressees"] = json!([]);
    accepted(&fixture.api.call("thread_post", post).await?);
    let mut latest = fetch(&selected, &reader, None, 100);
    latest["readMode"] = json!("latest");
    let shown = fixture.api.call("inbox_fetch", latest).await?;
    let ids = shown
        .pointer("/result/page/records")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("latest records: {shown}"))?
        .iter()
        .filter_map(|row| row.pointer("/post/postId"))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(ids.len(), 2, "{shown}");
    assert!(ids.contains(&json!(roots[0].0)));
    assert!(ids.contains(&json!(reply)));
    assert!(!ids.contains(&json!(roots[1].0)));
    let unread = fixture
        .api
        .call("inbox_fetch", fetch(&selected, &reader, None, 100))
        .await?;
    assert!(
        unread
            .pointer("/result/page/records")
            .and_then(Value::as_array)
            .ok_or("unread records")?
            .iter()
            .any(|row| row.pointer("/post/postId") == Some(&json!(reply))),
        "{unread}"
    );
    let strict=fixture.api.call("post_list",json!({"scope":{"kind":"project","projectId":selected},"selection":{"kind":"latest"},"page":{}})).await?;
    let strict_ids = strict
        .pointer("/result/page/records")
        .and_then(Value::as_array)
        .ok_or("strict records")?;
    assert_eq!(strict_ids.len(), 1);
    assert_eq!(strict_ids[0]["postId"], json!(roots[0].0));
    fixture.stop().await
}
