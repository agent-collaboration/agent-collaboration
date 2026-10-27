#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! Same-project cooldown uses immutable recorded posts; another project is independent.
use message_board::*;
use serde_json::json;
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError, accepted};

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove the test contract; Result propagates fixture errors.
async fn top_level_cooldown_replays_remaining_time_and_does_not_cross_projects()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let (other, _) = fixture.project().await?;
    let create = |project: &ProjectId, post: &PostId| {
        let mut request = fixture.header();
        request["postId"] = json!(post);
        request["projectId"] = json!(project);
        request["about"] = json!({"kind":"project","subject":{"line":"Cooldown root"}});
        request["role"] = json!(null);
        request["watch"] = json!(false);
        request["text"] = json!("root");
        request["references"] = json!([]);
        request["addressees"] = json!([]);
        request
    };
    let first = PostId::generate();
    accepted(
        &fixture
            .api
            .call("thread_create", create(&project, &first))
            .await?,
    );
    let second = create(&project, &PostId::generate());
    let rejected = fixture.api.call("thread_create", second.clone()).await?;
    assert_eq!(
        rejected.pointer("/result/result/decision/kind"),
        Some(&json!("rejected")),
        "{rejected}"
    );
    let reason = rejected
        .pointer("/result/result/decision/reasons/0")
        .ok_or("missing refusal")?;
    assert_eq!(reason["kind"], json!("postCooldown"));
    assert!(
        reason
            .pointer("/reason/retryAfterSeconds")
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|remaining| (1..=60).contains(&remaining)),
        "{reason}"
    );
    let replay = fixture.api.call("thread_create", second).await?;
    assert_eq!(rejected["result"]["result"], replay["result"]["result"]);
    accepted(
        &fixture
            .api
            .call("thread_create", create(&other, &PostId::generate()))
            .await?,
    );
    let mut reply = create(&project, &PostId::generate());
    let fields = reply.as_object_mut().ok_or("reply fields")?;
    for field in ["projectId", "about", "role", "watch"] {
        fields.remove(field);
    }
    reply["placement"] = json!({"kind":"thread","thread":{"thread":ThreadId::from(first)}});
    accepted(&fixture.api.call("thread_post", reply).await?);
    fixture.stop().await
}
