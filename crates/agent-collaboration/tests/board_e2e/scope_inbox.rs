//! BR02/06 / VD §5a U9,U10,U14,U42,U47: exact scope and ACK atomicity through real listeners.
use super::fixture::{BoardFixture, at, fields, header, human, identity};
use codex_router_test_support::FixtureResult;
use serde_json::{Value, json};

pub fn post_ids(page: &Value) -> FixtureResult<Vec<Value>> {
    at(page, "/page/records")?
        .as_array()
        .ok_or("inbox rows absent")?
        .iter()
        .map(|row| at(row, "/post/postId").cloned())
        .collect()
}

pub async fn foreign_watch(fixture: &mut BoardFixture) -> FixtureResult {
    const READER_WATCHES: &str = "SELECT thread_id AS value FROM thread_watches WHERE reader_actor=(SELECT actor_id FROM actors WHERE json_extract(canonical_identity,'$.kind')='human' AND json_extract(canonical_identity,'$.humanId')='rust-e2e-foreign-watch-reader')";
    // Wrong builds: plain project interest covers thread activity, or a foreign thread watch loses context.
    fixture.create().await?;
    let reader = human("foreign-watch-reader");
    fixture.initialize(&reader, &fixture.project).await?;
    let (_, foreign) = fixture.foreign_project().await?;
    let thread = fixture.thread(&foreign, &fixture.owner).await?;
    fixture
        .accept(
            "project_watch",
            fields(header(&reader), json!({"projectId":foreign}))?,
        )
        .await?;
    fixture
        .accept(
            "thread_resolve",
            fields(
                header(&fixture.owner),
                json!({
        "thread":{"project":foreign,"thread":thread},"reason":"done"}),
            )?,
        )
        .await?;
    assert_eq!(
        fixture
            .inbox(&reader, json!({}))
            .await?
            .pointer("/page/records"),
        Some(&json!([]))
    );
    fixture.stop_host().await?;
    assert!(
        super::records_reopen::stopped_project_values(fixture, &foreign, READER_WATCHES)
            .await?
            .is_empty()
    );
    fixture.reopen_host().await?;
    fixture
        .accept(
            "thread_watch",
            fields(
                header(&reader),
                json!({"thread":{"project":foreign,"thread":thread}}),
            )?,
        )
        .await?;
    fixture
        .accept(
            "thread_unresolve",
            fields(
                header(&fixture.owner),
                json!({"thread":{"project":foreign,"thread":thread}}),
            )?,
        )
        .await?;
    let answer = fixture.inbox(&reader, json!({})).await?;
    let rows = at(&answer, "/page/records")?
        .as_array()
        .ok_or("inbox rows")?;
    assert_eq!(rows.len(), 1);
    let row = rows.first().ok_or("foreign cause absent")?;
    assert_eq!(
        json!([
            row.get("kind"),
            row.get("projectId"),
            row.get("threadId"),
            row.get("state")
        ]),
        json!(["threadStateChanged", foreign, thread, "open"])
    );
    fixture.stop_host().await?;
    assert_eq!(
        super::records_reopen::stopped_project_values(fixture, &foreign, READER_WATCHES).await?,
        vec![thread]
    );
    Ok(())
}

struct AckCase {
    reader: Value,
    expected: Vec<Value>,
    entries: Value,
}

pub async fn semantic_ack(fixture: &mut BoardFixture) -> FixtureResult {
    // Wrong build: duplicated semantic scopes consume either request ID or advance unread coverage.
    fixture.create().await?;
    let case = prepare_ack(fixture).await?;
    refuse_duplicate_ack(fixture, &case).await?;
    assert_no_ack_decision(fixture, &case).await?;
    accept_one_ack(fixture, &case).await
}

async fn prepare_ack(fixture: &BoardFixture) -> FixtureResult<AckCase> {
    let thread = fixture.thread(&fixture.project, &fixture.owner).await?;
    let reader = human("ack-reader");
    fixture.initialize(&reader, &fixture.project).await?;
    fixture
        .accept(
            "thread_watch",
            fields(
                header(&reader),
                json!({"thread":{"project":fixture.project,"thread":thread}}),
            )?,
        )
        .await?;
    let mut posts = Vec::new();
    for number in 0..2 {
        posts.push(
            fixture
                .reply(
                    &fixture.project,
                    &thread,
                    &format!("unread verification {number}"),
                )
                .await?,
        );
    }
    let expected = posts
        .iter()
        .map(|post| at(post, "/postId").cloned())
        .collect::<FixtureResult<Vec<_>>>()?;
    assert_eq!(
        post_ids(&fixture.inbox(&reader, json!({})).await?)?,
        expected
    );
    let mut seen = Vec::new();
    let mut cursor = Value::Null;
    for _ in 0..3 {
        let page = fixture
            .inbox(&reader, json!({"page":{"limit":1,"cursor":cursor}}))
            .await?;
        seen.extend(post_ids(&page)?);
        cursor = at(&page, "/page/nextCursor")?.clone();
        if cursor.is_null() {
            break;
        }
    }
    assert_eq!(seen, expected);
    assert!(cursor.is_null());
    let last = posts.last().ok_or("last post")?;
    let shown = fixture
        .host()?
        .mcp("post_show", json!({"postId":at(last,"/postId")?}))
        .await?;
    let placement: message_board::RecordPlacement =
        serde_json::from_value(at(&shown, "/position")?.clone())?;
    let through = serde_json::to_value(
        placement
            .project_cursor()
            .ok_or("post has no project cursor")?,
    )?;
    let entries = json!([{"requestId":identity(),"scope":{"kind":"thread","thread":{"thread":thread}},"through":through},
        {"requestId":identity(),"scope":{"kind":"thread","thread":{"project":fixture.project,"thread":thread}},"through":through}]);
    Ok(AckCase {
        reader,
        expected,
        entries,
    })
}

async fn refuse_duplicate_ack(fixture: &BoardFixture, case: &AckCase) -> FixtureResult {
    let AckCase {
        reader,
        entries,
        expected,
    } = case;
    let refused = fixture
        .host()?
        .mcp_reply(
            "inbox_acknowledge",
            json!({"protocolVersion":1,"actor":reader,
        "actingFor":null,"entries":entries}),
        )
        .await?;
    assert!(refused.is_error);
    assert_eq!(
        json!([
            refused.value.get("mcpResult"),
            refused.value.get("kind"),
            refused.value.get("stage"),
            refused.value.pointer("/details/kind"),
            refused.value.pointer("/details/field")
        ]),
        json!([
            "error",
            "invalidField",
            "validation",
            "fieldConstraint",
            "request"
        ])
    );
    assert_eq!(
        post_ids(&fixture.inbox(reader, json!({})).await?)?,
        *expected
    );
    Ok(())
}

async fn assert_no_ack_decision(fixture: &mut BoardFixture, case: &AckCase) -> FixtureResult {
    let entries = &case.entries;
    let stopped = fixture
        .host
        .take()
        .ok_or("ACK readback Host absent")?
        .stop_for_readback()
        .await?;
    let relative = std::path::Path::new("projects")
        .join(&fixture.project)
        .join("e1.sqlite");
    let ledger = super::records_reopen::stopped_database_values(
        &stopped,
        &relative,
        "SELECT request_id AS value FROM request_decisions",
    )
    .await?;
    for entry in entries.as_array().ok_or("ACK entries")? {
        assert!(
            !ledger.contains(
                &at(entry, "/requestId")?
                    .as_str()
                    .ok_or("request ID")?
                    .to_owned()
            )
        );
    }
    fixture.reopen_host().await?;
    Ok(())
}

async fn accept_one_ack(fixture: &BoardFixture, case: &AckCase) -> FixtureResult {
    let AckCase {
        reader, entries, ..
    } = case;
    let entry = entries
        .as_array()
        .and_then(|entries| entries.first())
        .ok_or("first ACK")?;
    let reply = fixture
        .host()?
        .mcp(
            "inbox_acknowledge",
            json!({"protocolVersion":1,"actor":reader,
        "actingFor":null,"entries":[entry]}),
        )
        .await?;
    assert_eq!(
        reply.pointer("/outcomes/0/outcome/reply/result/decision/kind"),
        Some(&json!("accepted"))
    );
    let empty = fixture
        .cli(&[
            "inbox",
            "fetch",
            "--scope",
            "workspace",
            "--workspace-id",
            &fixture.workspace,
            "--reader",
            &reader.to_string(),
        ])
        .await?;
    assert_eq!(empty.pointer("/page/records"), Some(&json!([])));
    Ok(())
}
