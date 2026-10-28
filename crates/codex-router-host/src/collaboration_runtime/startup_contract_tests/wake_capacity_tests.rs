use super::{BOUND, client, start_host, target};
use collaboration_mail_protocol::{MailSender, WakeBody};
use collaboration_protocol::{OperationId, WakeMutationRequest, WakeSendRequest, WakeShowRequest};
use fleet_sync_protocol::{DeliveryItem, ItemId, Recipient};
use serde_json::json;

#[tokio::test]
async fn host_inbound_wake_waits_exhaust_outbound_wait_capacity() {
    let fixture = start_host(false).await;
    let api = client(&fixture).await;
    let inbound_target = target(&fixture.runtime, "inbound-capacity");
    let mut wakes = Vec::new();
    for recipient in [
        inbound_target.clone(),
        target(&fixture.runtime, "outbound-capacity"),
    ] {
        let request: WakeSendRequest = serde_json::from_value(json!({
            "operationId":OperationId::generate(),"requestedBy":{"kind":"owner"},"replace":false,
            "message":{"target":recipient,"content":{"kind":"humanUser","text":"Host capacity"},"delivery":"auto"},
            "timing":{"kind":"after","seconds":3600},"expiry":{"kind":"none"}
        })).expect("wake request");
        wakes.push(
            api.send_wakeup(request)
                .await
                .expect("wake created by Host API"),
        );
    }
    let inbound = wakes.remove(0);
    let outbound = wakes.remove(0);
    // Capture the active cursor first; the later wait must cross the real API even after pause.
    let waiting = client(&fixture)
        .await
        .subscribe_wakeup(WakeShowRequest {
            wakeup_id: outbound.definition.wakeup_id.clone(),
        })
        .await
        .expect("outbound active subscription");
    api.pause_wakeup(WakeMutationRequest {
        operation_id: OperationId::generate(),
        wakeup_id: outbound.definition.wakeup_id,
    })
    .await
    .expect("paused outbound answer");
    let mail = fixture
        .runtime
        .startup_test_mail
        .as_ref()
        .and_then(std::sync::Weak::upgrade)
        .expect("actual Host inbound runtime");
    let body: WakeBody = serde_json::from_value(json!({
        "requestedBy":{"kind":"owner"},"request":{"kind":"wait", "wakeupId":inbound.definition.wakeup_id,
        "after":null,"deadline":(chrono::Utc::now()+chrono::Duration::seconds(60)).to_rfc3339_opts(chrono::SecondsFormat::Millis,true)}
    })).expect("inbound wait");
    let mut inbound_waits = tokio::task::JoinSet::new();
    for _ in 0..16 {
        let item = DeliveryItem::<collaboration_contracts::RouterDeliveryTypes>::new(
            ItemId::new(uuid::Uuid::now_v7()).expect("mail item"),
            MailSender::HumanUser {
                installation: String::from(fixture.runtime.service_id().clone())
                    .try_into()
                    .expect("sender"),
            },
            Recipient {
                peer: fixture.runtime.installation_peer(),
                address: serde_json::from_value(
                    serde_json::to_value(&inbound_target).expect("target"),
                )
                .expect("recipient"),
            },
            None,
            board_sync_protocol::BoardDeliveryBody::Wake(body.clone()),
        )
        .expect("inbound envelope");
        let mail = mail.clone();
        let body = body.clone();
        inbound_waits
            .spawn(async move { mail.test_support_deliver_inbound_wake(&item, &body).await });
    }
    tokio::time::timeout(BOUND, async {
        while mail.test_support_available_wake_permits() != Some(0) {
            assert!(
                inbound_waits.try_join_next().is_none(),
                "an inbound wait was refused before capacity filled"
            );
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("all inbound permits admitted");
    let outcome = tokio::time::timeout(BOUND, waiting.wait_until_first_fire())
        .await
        .expect("outbound admission answers without waiting for a timer");
    assert!(
        matches!(
            outcome,
            Err(collaboration_client::WakeWaitError::Unavailable { .. })
        ),
        "Host inbound admission must exhaust outbound capacity, got {outcome:?}"
    );
    assert!(
        inbound_waits.try_join_next().is_none(),
        "inbound waits must remain admitted until cancellation"
    );
    inbound_waits.abort_all();
    while inbound_waits.join_next().await.is_some() {}
    drop(mail);
    drop(api);
    fixture.runtime.shutdown().await.expect("Host shutdown");
}
