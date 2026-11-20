//! A wake request from another installation, admitted to real recipient custody, is settled by
//! the real local delivery call as a refusal: this build keeps no local wakes.
//!
//! Admission runs through the actual Inbox; the delivery runs through `MailDeliveryRuntime`'s own
//! engine and call router, so the wake routing in `local_calls.rs` is on the path. Only the native
//! client stands in, and the wake must never reach it.
use super::native::Runtime;
use collaboration_contracts::RouterDeliveryTypes;
use collaboration_mail_protocol::{
    CarrierAcceptance, MailSender, RemoteWakeMutationState, RemoteWakeRefusal, RemoteWakeResult,
    WakeBody,
};
use fleet_sync_protocol::{DeliveryItem, ItemId, Recipient, RecipientOutcome};
use router_clock::RuntimeClock as _;
use serde_json::json;

const SENDER_INSTALLATION: &str = "00000000-0000-4000-8000-000000000001";

fn inbound_wake(runtime: &Runtime) -> DeliveryItem<RouterDeliveryTypes> {
    let deadline = runtime.fixture.host.clock.utc_now() + chrono::Duration::seconds(10);
    let body: WakeBody = serde_json::from_value(json!({
        "requestedBy":{"kind":"owner"},"request":{"kind":"wait",
        "wakeupId":"019f0000-0000-7000-8000-0000000000aa","after":null,
        "deadline":deadline.to_rfc3339_opts(chrono::SecondsFormat::Millis,true)}
    }))
    .expect("inbound wake request");
    DeliveryItem::new(
        ItemId::new(uuid::Uuid::now_v7()).expect("mail item"),
        MailSender::HumanUser {
            installation: SENDER_INSTALLATION.to_owned().try_into().expect("sender"),
        },
        Recipient {
            peer: runtime.fixture.host.config.local.owner,
            address: serde_json::from_value(json!({
                "endpoint":{"serviceId":SENDER_INSTALLATION,"endpointId":"codex-local"},
                "sessionId":"wake-target"
            }))
            .expect("mail recipient"),
        },
        None,
        board_sync_protocol::BoardDeliveryBody::Wake(body),
    )
    .expect("wake envelope")
}

/// Admits one inbound wake, optionally reopens the store so the item is read back from disk, and
/// drives the real local delivery task. Returns the runtime and the item's id.
async fn admit_and_deliver(reopen_after_admission: bool) -> (Runtime, ItemId) {
    let runtime = Runtime::new().await;
    let item = inbound_wake(&runtime);
    let id = item.id();
    runtime
        .fixture
        .receive_items(vec![item])
        .await
        .expect("the inbox admits the wake");
    let runtime = if reopen_after_admission {
        runtime.reopen().await
    } else {
        runtime
    };
    let mail = runtime.local_mail_runtime();
    mail.drive_due_for_test()
        .await
        .expect("the local delivery task runs");
    drop(mail);
    (runtime, id)
}

#[tokio::test]
async fn an_admitted_inbound_wake_settles_as_refused_with_no_local_mutation_and_no_native_push() {
    for reopen_after_admission in [false, true] {
        // Arrange and act: a fresh item, and an item stored before a restart.
        let (runtime, id) = admit_and_deliver(reopen_after_admission).await;

        // Assert: terminal custody carries the refusal, not a retry or an uncertain outcome.
        let outcome = runtime.fixture.local_outcome(id).await;
        let Some(RecipientOutcome::Delivered {
            acceptance: CarrierAcceptance::Wake(answer),
        }) = outcome
        else {
            panic!("reopen={reopen_after_admission}: wake custody is {outcome:?}");
        };
        assert!(
            matches!(
                *answer,
                RemoteWakeResult::Refused {
                    reason: RemoteWakeRefusal::AutomationUnavailable,
                    mutation: RemoteWakeMutationState::None,
                }
            ),
            "reopen={reopen_after_admission}: {answer:?}"
        );
        // No push row and no native call: the wake is answered, never turned into a message.
        let push_id = id.as_uuid().to_string().try_into().expect("push id");
        assert!(
            runtime
                .fixture
                .owner
                .get_push_record(&push_id)
                .await
                .expect("push read")
                .is_none(),
            "reopen={reopen_after_admission}: a wake produced a native push"
        );
        assert_eq!(
            runtime.client.calls(id),
            0,
            "reopen={reopen_after_admission}"
        );
        runtime.close().await;
    }
}
