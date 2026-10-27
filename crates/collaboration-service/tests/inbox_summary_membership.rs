//! PD1 E17 rebinding: marks count as membership; an unwatch alone creates no interest.
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
use message_board::*;
use serde_json::{Value, json};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn acknowledgement_counts_but_unwatch_alone_does_not_create_summary_membership()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let post = PostId::generate();
    let mut request = fixture.header();
    request.as_object_mut().ok_or("request")?.extend(json!({"projectId":project,"postId":post,"about":{"kind":"project","subject":{"line":"Fixture discussion"}},"text":"root","references":[],"role":null,"watch":false,"addressees":[]}).as_object().ok_or("fields")?.clone());
    accepted(&fixture.api.call("thread_create", request).await?);
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project missing")?;
    let mut snapshot = store.snapshot().await?;
    let Some(Record::Post(root)) = snapshot.record(&post.clone().into()).await? else {
        return Err("root missing".into());
    };
    snapshot.finish().await?;
    let through = root
        .position
        .project_cursor()
        .ok_or("root cursor not project qualified")?;
    let acknowledging = json!({"kind":"human","humanId":"ack-reader"});
    let request =
        json!({"reader":acknowledging,"unreadOnly":false,"page":{"limit":100,"cursor":null}});
    assert_eq!(
        fixture
            .api
            .call("inbox_workspaces", request.clone())
            .await?
            .pointer("/result/page/records"),
        Some(&json!([]))
    );
    let reply=fixture.api.call("inbox_acknowledge",json!({"protocolVersion":1,"actor":acknowledging,"actingFor":null,"entries":[{"requestId":RequestId::generate(),"scope":{"kind":"project","projectId":project},"through":through}]})).await?;
    assert_eq!(
        reply.pointer("/result/outcomes/0/outcome/reply/result/decision/kind"),
        Some(&json!("accepted")),
        "{reply}"
    );
    let summaries = fixture.api.call("inbox_workspaces", request).await?;
    let records = summaries
        .pointer("/result/page/records")
        .and_then(Value::as_array)
        .ok_or("summaries missing")?;
    assert_eq!(records.len(), 1);
    assert_eq!(
        records.first().and_then(|row| row.get("workspaceId")),
        Some(&json!(fixture.workspace))
    );
    assert_eq!(
        records.first().and_then(|row| row.get("hasUnread")),
        Some(&json!(false))
    );
    let unwatching = json!({"kind":"human","humanId":"unwatch-reader"});
    let mut unwatch = fixture.header();
    unwatch.as_object_mut().ok_or("request")?.extend(
        json!({"thread":{"thread":post},"actor":unwatching})
            .as_object()
            .ok_or("fields")?
            .clone(),
    );
    accepted(&fixture.api.call("thread_unwatch", unwatch).await?);
    assert_eq!(
        fixture
            .api
            .call(
                "inbox_workspaces",
                json!({"reader":unwatching,"unreadOnly":false,"page":{"limit":100,"cursor":null}})
            )
            .await?
            .pointer("/result/page/records"),
        Some(&json!([]))
    );
    let mut read = store.snapshot().await?;
    let marks = read.records(RecordKind::ReadMark).await?;
    let unwatching: Identity = serde_json::from_value(unwatching)?;
    assert!(
        !marks
            .iter()
            .any(|record| matches!(record,Record::ReadMark(mark) if mark.reader==unwatching))
    );
    read.finish().await?;
    fixture.stop().await
}
