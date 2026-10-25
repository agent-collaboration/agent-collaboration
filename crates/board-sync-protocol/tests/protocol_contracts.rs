#![allow(clippy::unwrap_used)]

use board_sync_protocol::*;
use fleet_sync_protocol as wire;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

const PROJECT: &str = "019a1234-5678-7000-8000-000000000001";
const REQUEST: &str = "019a1234-5678-7000-8000-000000000002";

fn mapping<Surface, Wire>(cases: Vec<(Value, Value)>)
where
    Surface: Serialize + DeserializeOwned + From<Wire> + Into<Wire>,
    Wire: Serialize + DeserializeOwned,
{
    for (wire_json, expected_surface) in cases {
        let wire_value: Wire = serde_json::from_value(wire_json.clone()).unwrap();
        let surface = Surface::from(wire_value);
        assert_eq!(serde_json::to_value(&surface).unwrap(), expected_surface);
        let decoded: Surface = serde_json::from_value(expected_surface).unwrap();
        let restored: Wire = decoded.into();
        assert_eq!(serde_json::to_value(restored).unwrap(), wire_json);
    }
}

#[test]
fn piece_refusal_surface_names_keep_acknowledgements_epochs_and_details() {
    mapping::<BoardPieceRefusal, wire::PieceRefusal>(vec![
        (
            json!({"kind":"gap","value":{"acked":{"epoch":7,"sequence":11}}}),
            json!({"kind":"positionGap","value":{"acked":{"epoch":7,"sequence":11}}}),
        ),
        (
            json!({"kind":"copyFull"}),
            json!({"kind":"centralCopyFull"}),
        ),
        (
            json!({"kind":"notWriter","value":{"current":7}}),
            json!({"kind":"notWriter","value":{"current":7}}),
        ),
    ]);
}

#[test]
fn registry_and_registration_names_keep_rejection_reasons() {
    mapping::<BoardLeaseRefusal, wire::LeaseRefusal>(vec![(
        json!({"kind":"epochUnprovable"}),
        json!({"kind":"registryEpochUnproven"}),
    )]);
    mapping::<BoardRegistrationRefusal, wire::RegistrationRefusal>(vec![(
        json!({"kind":"databaseRejected","value":{"reason":{"code":"archived","detail":"board closed"}}}),
        json!({"kind":"projectRejected","value":{"reason":{"code":"archived","detail":"board closed"}}}),
    )]);
}

#[test]
fn claim_keeps_expiry_and_seed_position_when_granted_becomes_claimed() {
    mapping::<BoardClaimAnswer, wire::HolderChangeAnswer>(vec![(
        json!({"kind":"granted","value":{"epoch":7,"expiresAt":12345,"start":{"epoch":6,"sequence":99}}}),
        json!({"kind":"claimed","value":{"epoch":7,"expiresAt":12345,"start":{"epoch":6,"sequence":99}}}),
    )]);
}

#[test]
fn holder_observations_and_frozen_refusal_detail_are_never_relabelled() {
    mapping::<BoardHolderChangeRefusal, wire::HolderChangeRefusal>(vec![
        (
            json!({"kind":"holderStale","value":{"lastRenewedAt":12345}}),
            json!({"kind":"holderUnreachable","value":{"lastRenewedAt":12345}}),
        ),
        (
            json!({"kind":"holderUnreachable","value":{"lastSeen":12340}}),
            json!({"kind":"holderUnreachable","value":{"lastSeen":12340}}),
        ),
        (
            json!({"kind":"alreadyHeld","value":{"holder":PROJECT}}),
            json!({"kind":"alreadyHeld","value":{"holder":PROJECT}}),
        ),
        (
            json!({"kind":"migrationPending","value":{"migration":3,"state":"failed"}}),
            json!({"kind":"migrationPending","value":{"migration":3,"state":"failed"}}),
        ),
        (
            json!({"kind":"epochUnprovable"}),
            json!({"kind":"registryEpochUnproven"}),
        ),
    ]);
}

#[test]
fn delivery_names_keep_custody_and_cancel_effect_distinct() {
    mapping::<
        BoardDeliveryStatus,
        wire::DeliveryStatus<collaboration_mail_protocol::CarrierAcceptance>,
    >(vec![
        (json!({"kind":"atRecipient"}), json!({"kind":"atMachine"})),
        (
            json!({"kind":"delivered","value":{"acceptance":"peerMessageWritten"}}),
            json!({"kind":"delivered","value":{"acceptance":"peerMessageWritten"}}),
        ),
    ]);
    mapping::<BoardCancelResult, wire::CancelResult>(vec![
        (json!({"kind":"pending"}), json!({"kind":"cancelPending"})),
        (
            json!({"kind":"tooLate","value":{"at":"uncertain"}}),
            json!({"kind":"tooLate","value":{"at":"uncertain"}}),
        ),
    ]);
}

#[test]
fn every_request_family_round_trips_inside_the_real_library_frame() {
    for body in [
        json!({"kind":"forwardedPost","value":{"project":PROJECT,"post":{"kind":"threadCreate","body":{"requestId":REQUEST}}}}),
        json!({"kind":"fleet","value":{"requestId":REQUEST,"operation":{"kind":"workspaceArchive"}}}),
        json!({"kind":"read","value":{"scope":{"kind":"requestEffects","value":{"workspace":PROJECT,"request":REQUEST,"after":{"kind":"fleet","ordinal":0}}},"cursor":null}}),
        json!({"kind":"archiveSeen","value":{"board":PROJECT,"project":PROJECT,"position":{"epoch":7,"sequence":11}}}),
    ] {
        let expected = json!({"kind":"request","value":{"frame":REQUEST,"body":{"kind":"consumer","value":body}}});
        let frame: wire::ClientFrame<BoardConsumerProtocol> =
            serde_json::from_value(expected.clone()).unwrap();
        assert_eq!(serde_json::to_value(frame).unwrap(), expected);
    }
}

#[test]
fn annotation_never_substitutes_a_time_for_imported_history() {
    let expected =
        json!({"kind":"projectSummary","value":{"latestRecordAt":{"kind":"legacyUnknown"}}});
    let annotation: BoardAnnotation = serde_json::from_value(expected.clone()).unwrap();
    assert_eq!(serde_json::to_value(annotation).unwrap(), expected);
}

#[test]
fn opaque_payload_decodes_only_at_its_typed_receiving_owner() {
    let source = message_board::RecordTime::LegacyUnknown;
    let payload = BoardPayload::encode(&source).unwrap();
    let decoded: message_board::RecordTime = payload.decode().unwrap();
    assert_eq!(decoded, source);
    let wrong = BoardPayload::encode(&json!({"kind":"invented"})).unwrap();
    assert!(wrong.decode::<message_board::RecordTime>().is_err());
}

#[test]
fn answers_server_calls_notices_and_annotations_use_the_real_frame_codec() {
    for body in [
        json!({"kind":"forwardedPost","value":{"kind":"answered","value":{"reply":{"result":"accepted"}}}}),
        json!({"kind":"fleet","value":{"requestId":REQUEST,"decision":{"kind":"accepted"}}}),
        json!({"kind":"read","value":{"kind":"requestEffects","value":{"result":{"kind":"refused","reason":{"kind":"unknownRequest"}},"asOf":"2026-10-18T08:00:00Z"}}}),
        json!({"kind":"read","value":{"kind":"fleetFeed","value":{"kind":"page","value":{"records":[],"through":0,"head":0,"asOf":"2026-10-18T08:00:00Z"}}}}),
        json!({"kind":"archiveSeen","value":{"kind":"recorded"}}),
    ] {
        let expected = json!({"kind":"answer","value":{"frame":REQUEST,"body":{"kind":"consumer","value":body}}});
        let frame: wire::ServerFrame<BoardConsumerProtocol> =
            serde_json::from_value(expected.clone()).unwrap();
        assert_eq!(serde_json::to_value(frame).unwrap(), expected);
    }
    let call = json!({"kind":"forwardedPost","value":{"project":PROJECT,"origin":{"installation":"00000000-0000-4000-8000-000000000001","machine":"sender"},"post":{"kind":"threadPost","body":{"requestId":REQUEST}}}});
    let request: BoardServerRequest = serde_json::from_value(call.clone()).unwrap();
    assert_eq!(serde_json::to_value(request).unwrap(), call);
    let reply = json!({"kind":"forwardedPost","value":{"result":"accepted"}});
    let answer: BoardClientAnswer = serde_json::from_value(reply.clone()).unwrap();
    assert_eq!(serde_json::to_value(answer).unwrap(), reply);
    for notice in [
        json!({"kind":"fleetFeedAdvanced","value":{"through":11}}),
        json!({"kind":"leaseSuperseded","value":{"project":PROJECT,"epoch":7}}),
        json!({"kind":"leaseGranted","value":{"project":PROJECT,"epoch":7}}),
        json!({"kind":"boardArchiving","value":{"board":PROJECT}}),
    ] {
        let expected = json!({"kind":"notice","value":{"position":3,"body":{"kind":"consumer","value":{"position":73,"kind":notice}}}});
        let frame: wire::ServerFrame<BoardConsumerProtocol> =
            serde_json::from_value(expected.clone()).unwrap();
        assert_eq!(serde_json::to_value(frame).unwrap(), expected);
    }
}

#[test]
fn nested_claim_refusal_and_unknown_surface_names_fail_closed() {
    let removed_project_copy_tags_are_rejected = [
        serde_json::from_value::<BoardReadScope>(json!({
            "kind":"projectCopy","value":{"project":PROJECT}
        }))
        .is_err(),
        serde_json::from_value::<BoardReadPage>(json!({
            "kind":"projectCopy","value":{
                "kind":"moved","value":{"position":{"epoch":7,"sequence":11}}
            }
        }))
        .is_err(),
    ];
    assert_eq!(removed_project_copy_tags_are_rejected, [true, true]);
    mapping::<BoardClaimAnswer, wire::HolderChangeAnswer>(vec![(
        json!({"kind":"refused","value":{"kind":"holderStale","value":{"lastRenewedAt":12345}}}),
        json!({"kind":"refused","value":{"kind":"holderUnreachable","value":{"lastRenewedAt":12345}}}),
    )]);
    assert!(
        serde_json::from_value::<BoardHolderChangeRefusal>(json!({
            "kind":"holderUnreachable", "value":{"lastSeen":1,"lastRenewedAt":2}
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<BoardPieceRefusal>(
            json!({"kind":"gap","value":{"acked":{"epoch":7,"sequence":11}}})
        )
        .is_err()
    );
    assert!(serde_json::from_value::<BoardConsumerRequest>(json!({"kind":"invented"})).is_err());
}

#[test]
fn holder_observation_requires_exactly_one_named_observation() {
    for observation in [json!({}), json!({"lastSeen": 1, "lastRenewedAt": 2})] {
        assert!(serde_json::from_value::<HolderObservation>(observation).is_err());
    }
    let unknown_presence: HolderObservation =
        serde_json::from_value(json!({"lastSeen": null})).unwrap();
    assert_eq!(
        unknown_presence,
        HolderObservation::Presence { last_seen: None }
    );
}

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
