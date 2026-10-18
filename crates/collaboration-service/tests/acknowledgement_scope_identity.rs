//! Qualification changes routing, never the semantic identity of an ACK scope.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
#[path = "support/board_read_setup.rs"]
mod board_read_setup;
use board_fixture::{BoardFixture, TestError};
use board_read_setup::post_at_placement;
use message_board::*;
use serde_json::json;

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn duplicate_thread_spellings_reject_before_marks_or_either_ledger_key()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let root = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: project.clone(),
        },
        json!({"text":"ACK scope root"}),
    )
    .await?;
    let thread = ThreadId::from(root);
    post_at_placement(
        &fixture,
        Placement::Thread {
            thread_id: thread.clone(),
        },
        json!({"text":"unread reply"}),
    )
    .await?;
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    let before = read.records(RecordKind::ReadMark).await?;
    let head = read.high_water();
    read.finish().await?;
    let actor = json!({"kind":"human","humanId":"semantic ACK reader"});
    let bare = json!({"kind":"thread","thread":{"thread":thread}});
    let qualified = json!({"kind":"thread","thread":{"thread":thread,"project":project}});
    let other_qualified =
        json!({"kind":"thread","thread":{"thread":thread,"project":ProjectId::generate()}});
    let first = RequestId::generate();
    let mut all_keys = Vec::new();
    for (scopes, first_key) in [
        (vec![bare, qualified.clone()], first.clone()),
        (
            vec![qualified.clone(), other_qualified],
            RequestId::generate(),
        ),
    ] {
        let second = RequestId::generate();
        let keys = [first_key, second];
        let entries: Vec<_> = scopes.into_iter().zip(&keys).map(|(scope,key)|json!({"requestId":key,"resubmits":null,"observedEpoch":1,"scope":scope,"through":head})).collect();
        let response = fixture
            .api
            .call(
                "inbox_acknowledge",
                json!({"protocolVersion":1,"actor":actor,"actingFor":null,"entries":entries}),
            )
            .await?;
        assert_eq!(
            response.pointer("/error/data/kind"),
            Some(&json!("invalidField")),
            "{response}"
        );
        let mut read = store.snapshot().await?;
        assert_eq!(
            read.records(RecordKind::ReadMark).await?,
            before,
            "invalid batch changed replicated marks"
        );
        assert_eq!(read.high_water(), head, "invalid batch allocated activity");
        for key in &keys {
            assert!(
                read.record(&key.clone().into()).await?.is_none(),
                "invalid batch consumed request {key:?}"
            );
        }
        read.finish().await?;
        all_keys.extend(keys);
    }
    let response = fixture.api.call("inbox_acknowledge",json!({"protocolVersion":1,"actor":actor,"actingFor":null,"entries":[{"requestId":first,"resubmits":null,"observedEpoch":1,"scope":qualified,"through":head}]})).await?;
    assert_eq!(
        response.pointer("/result/outcomes/0/outcome/reply/result/decision/kind"),
        Some(&json!("accepted")),
        "{response}"
    );
    let mut read = store.snapshot().await?;
    assert!(
        matches!(read.record(&first.clone().into()).await?,Some(Record::Request(ledger)) if matches!(ledger.decision,RequestDecision::Accepted {..}))
    );
    let reader: Identity = serde_json::from_value(actor)?;
    assert_eq!(
        read.read_mark(&reader, &ReadScope::Thread { thread_id: thread })
            .await?
            .ok_or("accepted mark")?
            .through,
        head
    );
    for key in all_keys.into_iter().filter(|key| key != &first) {
        assert!(read.record(&key.into()).await?.is_none());
    }
    read.finish().await?;
    fixture.stop().await
}
