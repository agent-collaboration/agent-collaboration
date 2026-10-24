#![allow(clippy::unwrap_used)]

use board_sync_protocol::{BoardConsumerProtocol, BoardConsumerRequest, BoardDeliveryTypes};
use fleet_sync_protocol as wire;
use serde_json::{Value, json};

const ITEM: &str = "019a1234-5678-7000-8000-000000000001";
const TARGET: &str = "019a1234-5678-7000-8000-000000000002";
const PEER: &str = "019a1234-5678-7000-8000-000000000003";
const INSTALLATION: &str = "00000000-0000-4000-8000-000000000001";

fn recipient() -> wire::Recipient<message_board::SessionRef> {
    serde_json::from_value(json!({"peer":PEER,"address":{
        "endpoint":{"serviceId":INSTALLATION,"endpointId":"codex-local"},
        "sessionId":"recipient"
    }}))
    .unwrap()
}

fn envelope(sender: Value, cancel: bool) -> wire::DeliveryEnvelope<BoardDeliveryTypes> {
    let id = ITEM.to_owned().try_into().unwrap();
    let sender = serde_json::from_value(sender).unwrap();
    if cancel {
        wire::DeliveryEnvelope::Cancel(
            wire::DeliveryCancel::new(
                id,
                sender,
                recipient(),
                None,
                wire::CancelBody {
                    target: TARGET.to_owned().try_into().unwrap(),
                },
            )
            .unwrap(),
        )
    } else {
        let body = if matches!(
            &sender,
            collaboration_mail_protocol::MailSender::Project { .. }
        ) {
            json!({"kind":"notification","excerpt":"A reference","pointer":{"project":ITEM,"subject":{"kind":"task","id":TARGET},"position":{"epoch":7,"position":11}},"source":{"kind":"intent","intent":TARGET}})
        } else {
            json!({"kind":"message","text":"Hello","delivery":"queue","guard":null,"senderDisplayName":null,"replyTo":null})
        };
        wire::DeliveryEnvelope::Item(
            wire::DeliveryItem::new(
                id,
                sender,
                recipient(),
                None,
                serde_json::from_value(body).unwrap(),
            )
            .unwrap(),
        )
    }
}

fn submit(item: &wire::DeliveryEnvelope<BoardDeliveryTypes>, epoch: Value) -> Value {
    json!({"kind":"request","value":{"frame":ITEM,"body":{"kind":"consumer","value":{
        "kind":"mailSubmit","value":{"item":item,"originEpoch":epoch}
    }}}})
}

#[test]
fn mail_submit_carries_library_items_and_cancels_without_translating_the_digest() {
    for sender in [
        json!({"kind":"humanUser","installation":INSTALLATION}),
        json!({"kind":"session","endpoint":{"serviceId":INSTALLATION,"endpointId":"codex-local"},"sessionId":"sender"}),
        json!({"kind":"project","project":ITEM}),
    ] {
        let epoch = if sender["kind"] == "project" {
            json!(7)
        } else {
            Value::Null
        };
        for cancel in [false, true] {
            let item = envelope(sender.clone(), cancel);
            let expected = submit(&item, epoch.clone());
            let decoded = serde_json::from_value::<wire::ClientFrame<BoardConsumerProtocol>>(
                expected.clone(),
            );
            assert!(
                decoded.is_ok(),
                "library envelope was rejected: {}",
                decoded
                    .err()
                    .map(|error| error.to_string())
                    .unwrap_or_default()
            );
            assert_eq!(serde_json::to_value(decoded.unwrap()).unwrap(), expected);
        }
    }
}

#[test]
fn mail_submit_requires_project_epochs_and_forbids_them_for_other_senders() {
    for sender in [
        json!({"kind":"humanUser","installation":INSTALLATION}),
        json!({"kind":"session","endpoint":{"serviceId":INSTALLATION,"endpointId":"codex-local"},"sessionId":"sender"}),
        json!({"kind":"project","project":ITEM}),
    ] {
        let project = sender["kind"] == "project";
        for cancel in [false, true] {
            let item = envelope(sender.clone(), cancel);
            let mut invalid = submit(&item, if project { Value::Null } else { json!(7) });
            let error =
                serde_json::from_value::<wire::ClientFrame<BoardConsumerProtocol>>(invalid.clone())
                    .err()
                    .unwrap();
            assert!(error.to_string().contains("originEpoch"), "{error}");
            invalid["value"]["body"]["value"]["value"]
                .as_object_mut()
                .unwrap()
                .remove("originEpoch");
            assert_eq!(
                serde_json::from_value::<wire::ClientFrame<BoardConsumerProtocol>>(invalid).is_ok(),
                !project
            );
        }
    }
}

#[test]
fn mail_submit_validates_the_recipient_against_its_enrollment_without_rehashing() {
    let sender = json!({"kind":"humanUser","installation":INSTALLATION});
    let installation = INSTALLATION.to_owned().try_into().unwrap();
    let peer = PEER.to_owned().try_into().unwrap();
    let other_installation = "00000000-0000-4000-8000-000000000002"
        .to_owned()
        .try_into()
        .unwrap();
    let other_peer = TARGET.to_owned().try_into().unwrap();
    for cancel in [false, true] {
        let item = envelope(sender.clone(), cancel);
        let expected = serde_json::to_value(&item).unwrap();
        let request = BoardConsumerRequest::mail_submit(item, None, &installation, peer).unwrap();
        assert_eq!(
            serde_json::to_value(request).unwrap()["value"]["item"],
            expected
        );
        let error = BoardConsumerRequest::mail_submit(
            envelope(sender.clone(), cancel),
            None,
            &installation,
            other_peer,
        )
        .err()
        .unwrap();
        assert_eq!(error.field, "recipient.peer");
        let error = BoardConsumerRequest::mail_submit(
            envelope(sender.clone(), cancel),
            None,
            &other_installation,
            peer,
        )
        .err()
        .unwrap();
        assert_eq!(error.field, "recipient.address.endpoint.serviceId");
    }
}

#[test]
fn mail_submit_rejects_invalid_senders_and_changed_digests_at_the_frame_boundary() {
    for sender in [
        json!({"kind":"humanUser","installation":INSTALLATION}),
        json!({"kind":"session","endpoint":{"serviceId":INSTALLATION,"endpointId":"codex-local"},"sessionId":"sender"}),
        json!({"kind":"project","project":ITEM}),
    ] {
        let wrong_body = if sender["kind"] == "project" {
            json!({"kind":"message","text":"Hello","delivery":"queue","guard":null,"senderDisplayName":null,"replyTo":null})
        } else {
            serde_json::to_value(envelope(json!({"kind":"project","project":ITEM}), false)).unwrap()
                ["value"]["body"]
                .clone()
        };
        // The library can hash this typed item; the board must reject its sender/body pairing.
        let invalid = wire::DeliveryEnvelope::Item(
            wire::DeliveryItem::<BoardDeliveryTypes>::new(
                ITEM.to_owned().try_into().unwrap(),
                serde_json::from_value(sender.clone()).unwrap(),
                recipient(),
                None,
                serde_json::from_value(wrong_body).unwrap(),
            )
            .unwrap(),
        );
        let epoch = if sender["kind"] == "project" {
            json!(7)
        } else {
            Value::Null
        };
        let error = serde_json::from_value::<wire::ClientFrame<BoardConsumerProtocol>>(submit(
            &invalid,
            epoch.clone(),
        ))
        .err()
        .unwrap();
        assert!(error.to_string().contains("sender"), "{error}");
        let error = BoardConsumerRequest::mail_submit(
            invalid,
            if epoch.is_null() {
                None
            } else {
                Some(wire::Epoch::new(7).unwrap())
            },
            &INSTALLATION.to_owned().try_into().unwrap(),
            PEER.to_owned().try_into().unwrap(),
        )
        .err()
        .unwrap();
        assert_eq!(error.field, "sender");
        for cancel in [false, true] {
            let mut changed = submit(&envelope(sender.clone(), cancel), epoch.clone());
            changed["value"]["body"]["value"]["value"]["item"]["value"]["digest"] =
                json!("0".repeat(64));
            let error = serde_json::from_value::<wire::ClientFrame<BoardConsumerProtocol>>(changed)
                .err()
                .unwrap();
            assert!(error.to_string().contains("digest mismatch"), "{error}");
        }
    }
}

#[test]
fn cancel_is_unrepresentable_as_an_item_body_and_legacy_items_are_not_carried() {
    type Body = <BoardDeliveryTypes as wire::DeliveryTypes>::Body;
    assert!(serde_json::from_value::<Body>(json!({"kind":"cancel","target":TARGET})).is_err());
    let item = envelope(
        json!({"kind":"humanUser","installation":INSTALLATION}),
        false,
    );
    let mut legacy = submit(&item, Value::Null);
    legacy["value"]["body"]["value"]["value"]["item"] =
        serde_json::to_value(item).unwrap()["value"].clone();
    assert!(serde_json::from_value::<wire::ClientFrame<BoardConsumerProtocol>>(legacy).is_err());
}
