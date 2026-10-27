//! C5 leaves foreign recipients for P3 without a claim, ledger entry or feed write.
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
use collaboration_service::IntentReply;
use message_board::*;
use serde_json::json;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test-owned JSON fields and literal contract assertions."
)]
async fn explicit_and_role_resolved_foreign_sessions_are_deferred_without_writes()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let foreign: Identity = serde_json::from_value(json!({"kind":"session","session":{
        "endpoint":{"endpointId":"codex-local","serviceId":"00000000-0000-4000-8000-000000000099"},
        "sessionId":"foreign-recipient"
    }}))?;
    for via_role in [false, true] {
        let (project, workstream) = fixture.project().await?;
        if via_role {
            accepted(&fixture.api.call("role_grant", fixture.request(&project, json!({
                "kind":"roleGrant","subject":{"kind":"workstream","subject":workstream},
                "role":"implementer","holder":foreign,"model":null,"lineage":null,
                "expectedRevision":0,"ownerGrant":null
            }))).await?);
        }
        let mut request = fixture.header();
        request["projectId"] = json!(project);
        request["postId"] = json!(PostId::generate());
        request["about"] = json!({"kind":"workstream","subject":workstream});
        request["text"] = json!("foreign intent cause");
        request["references"] = json!([]);
        request["role"] = json!(null);
        request["watch"] = json!(false);
        request["addressees"] = json!([if via_role {
            NotifyTarget::Role(Assignment::Implementer(workstream))
        } else {
            NotifyTarget::Actor(foreign.clone())
        }]);
        accepted(&fixture.api.call("thread_create", request).await?);
        let store = fixture
            ._registry
            .existing_project(&project)
            .await?
            .ok_or("project missing")?;
        let mut before = store.snapshot().await?;
        let high_water = before.high_water();
        let ledger = before.records(RecordKind::Request).await?;
        let intents = before.records(RecordKind::NotifyIntent).await?;
        before.finish().await?;
        assert!(
            intents
                .iter()
                .any(|record| matches!(record, Record::NotifyIntent(intent)
            if matches!(intent.recipient(), IntentRecipient::Resolved(recipient)
                if recipient.actor.declared_identity().as_ref() == Some(&foreign))))
        );
        for record in &intents {
            let Record::NotifyIntent(intent) = record else {
                return Err("intent kind".into());
            };
            if !matches!(intent.recipient(), IntentRecipient::Resolved(recipient)
                if recipient.actor.declared_identity().as_ref() == Some(&foreign))
            {
                continue;
            }
            assert!(matches!(intent.state(), IntentState::Emitted {}));
            let result = fixture
                ._admission
                .claim_intent(&project, &intent.intent_id, None)
                .await?;
            let IntentReply::Deferred {
                recipient,
                replacements,
            } = result
            else {
                return Err("foreign recipient was claimed instead of deferred".into());
            };
            assert!(replacements.is_empty());
            assert_eq!(recipient.actor.declared_identity().as_ref(), Some(&foreign));
        }
        let mut after = store.snapshot().await?;
        assert_eq!(after.high_water(), high_water);
        assert_eq!(after.records(RecordKind::Request).await?, ledger);
        assert_eq!(after.records(RecordKind::NotifyIntent).await?, intents);
        after.finish().await?;
    }
    fixture.stop().await
}
