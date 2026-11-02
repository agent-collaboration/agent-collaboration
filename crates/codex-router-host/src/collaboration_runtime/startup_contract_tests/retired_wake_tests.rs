//! Legacy wire requests cannot reacquire local timer ownership after command cutover.
use super::{start_host, target};
use collaboration_mail_protocol::{
    CarrierAcceptance, MailSender, RemoteWakeMutationState, RemoteWakeRefusal, RemoteWakeResult,
    WakeBody,
};
use collaboration_protocol::{OperationId, WakeupId};
use fleet_sync_delivery::LocalOutcome;
use fleet_sync_protocol::{DeliveryItem, ItemId, Recipient};
use serde_json::json;

#[tokio::test]
async fn host_retired_inbound_wake_requests_are_refused_without_local_admission() {
    let fixture = start_host(false).await;
    let recipient = target(&fixture.runtime, "retired-inbound-wake");
    let wakeup_id = WakeupId::generate();
    let mail = fixture
        .runtime
        .startup_test_mail
        .as_ref()
        .and_then(std::sync::Weak::upgrade)
        .expect("actual Host mail consumer");
    for kind in ["send", "show", "pause", "resume", "cancel", "wait"] {
        let request = match kind {
            "send" => json!({"kind":kind,"operationId":OperationId::generate(),"replace":false,
                "message":{"target":recipient,"content":{"kind":"humanUser","text":"Never admitted"},"delivery":"auto"},
                "timing":{"kind":"interval","seconds":300},"expiry":{"kind":"none"}}),
            "show" => json!({"kind":kind,"wakeupId":wakeup_id}),
            "wait" => {
                json!({"kind":kind,"wakeupId":wakeup_id,"after":null,"deadline":"2030-01-01T00:00:00Z"})
            }
            _ => json!({"kind":kind,"operationId":OperationId::generate(),"wakeupId":wakeup_id}),
        };
        let body: WakeBody =
            serde_json::from_value(json!({"requestedBy":{"kind":"owner"},"request":request}))
                .expect("legacy wire request");
        let item = DeliveryItem::<collaboration_contracts::RouterDeliveryTypes>::new(
            ItemId::new(uuid::Uuid::now_v7()).expect("mail ID"),
            MailSender::HumanUser {
                installation: String::from(fixture.runtime.service_id().clone())
                    .try_into()
                    .expect("sender"),
            },
            Recipient {
                peer: fixture.runtime.installation_peer(),
                address: serde_json::from_value(serde_json::to_value(&recipient).expect("target"))
                    .expect("recipient"),
            },
            None,
            board_sync_protocol::BoardDeliveryBody::Wake(body.clone()),
        )
        .expect("legacy mail envelope");
        let result = mail.test_support_deliver_inbound_wake(&item, &body);
        assert!(
            matches!(result.outcome, LocalOutcome::Delivered { acceptance: CarrierAcceptance::Wake(answer) }
            if matches!(*answer, RemoteWakeResult::Refused { reason: RemoteWakeRefusal::AutomationUnavailable, mutation: RemoteWakeMutationState::None })),
            "legacy {kind} was admitted"
        );
    }
    let mut store = automation_storage::AutomationStore::open(
        &fixture.directory.path().join("automation.sqlite"),
    )
    .await
    .expect("actual Host store");
    let page = store
        .list_wakeups::<collaboration_protocol::SavedMessage>(None, 100)
        .await
        .expect("stored Wakes");
    assert!(
        page.records.is_empty() && page.next.is_none(),
        "retired mail created a local timer"
    );
    store.close().await.expect("close observation");
    drop(mail);
    fixture.runtime.shutdown().await.expect("Host shutdown");
}
