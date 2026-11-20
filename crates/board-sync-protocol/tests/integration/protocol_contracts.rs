use board_sync_protocol::*;
use fleet_sync_protocol as wire;
use serde_json::json;

const PROJECT: &str = "019a1234-5678-7000-8000-000000000001";
const REQUEST: &str = "019a1234-5678-7000-8000-000000000002";

#[test]
fn delivery_type_bounds_cover_the_largest_legal_session_identity() {
    use collaboration_mail_protocol::{CarrierAcceptance, MailSender};
    use wire::DeliveryTypes;

    let session: message_board::SessionRef = serde_json::from_value(json!({
        "endpoint":{"serviceId":PROJECT,"endpointId":"a".repeat(64)},
        "sessionId":"\u{0001}".repeat(4096)
    }))
    .unwrap();
    let address = serde_json::to_vec(&session).unwrap();
    let sender = serde_json::to_vec(&MailSender::Session(session)).unwrap();
    let installation = "00000000-0000-4000-8000-000000000001";
    let route: <BoardDeliveryTypes as wire::DeliveryTypes>::StatusRoute =
        installation.to_owned().try_into().unwrap();
    assert_eq!(serde_json::to_value(&route).unwrap(), json!(installation));
    assert_eq!(
        serde_json::to_vec(&route).unwrap().len(),
        BoardDeliveryTypes::MAX_STATUS_ROUTE_BYTES as usize
    );
    assert!(address.len() <= BoardDeliveryTypes::MAX_ADDRESS_BYTES as usize);
    assert!(sender.len() <= BoardDeliveryTypes::MAX_SENDER_BYTES as usize);
    for acceptance in [
        CarrierAcceptance::Started,
        CarrierAcceptance::Steered,
        CarrierAcceptance::StartedOrSteered,
        CarrierAcceptance::Queued,
        CarrierAcceptance::PeerMessageWritten,
    ] {
        let encoded = serde_json::to_vec(&acceptance).unwrap();
        assert!(encoded.len() <= BoardDeliveryTypes::MAX_ACCEPTANCE_BYTES as usize);
    }
}

#[test]
fn delivery_acceptance_reserves_are_small_for_ordinary_mail_and_large_only_for_wakes() {
    use wire::DeliveryTypes;

    const { assert!(BoardDeliveryTypes::PER_ITEM_SETTLEMENT) };
    assert_eq!(BoardDeliveryTypes::MAX_ACCEPTANCE_BYTES, 1024 * 1024 + 1024);
    for (body, expected) in [
        (
            json!({"kind":"message","text":"ordinary","delivery":"auto","guard":null,"senderDisplayName":null,"replyTo":null}),
            64,
        ),
        (
            json!({"kind":"notification","excerpt":"notice","pointer":{"project":PROJECT,"subject":{"kind":"task","id":REQUEST},"position":{"epoch":7,"position":11}},"source":{"kind":"intent","intent":REQUEST}}),
            64,
        ),
        (
            json!({"kind":"wake","request":{"kind":"show","wakeupId":REQUEST},"requestedBy":{"kind":"owner"}}),
            1024 * 1024 + 1024,
        ),
    ] {
        let body: BoardDeliveryBody = serde_json::from_value(body).unwrap();
        assert_eq!(
            BoardDeliveryTypes::max_acceptance_bytes_for(&body),
            expected
        );
    }
}
