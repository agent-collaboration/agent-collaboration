use super::*;
use fleet_sync_protocol::DeliveryTypes;
use serde_json::json;

const ID: &str = "019f0000-0000-7000-8000-000000000001";
const SESSION_BYTES: &str = r#"{"endpoint":{"serviceId":"019f0000-0000-7000-8000-000000000001","endpointId":"codex-local"},"sessionId":"retained-d1"}"#;

#[test]
fn fleet_create_command_has_the_contracted_acceptance_budget() {
    let body: BoardDeliveryBody = serde_json::from_value(json!({
        "kind":"createSession", "requestId":ID,
        "requester":serde_json::from_str::<serde_json::Value>(SESSION_BYTES).unwrap(),
        "command":{"endpoint":"codex-local", "workingDirectory":"/tmp", "access":"workspace-write",
            "model":null,"effort":null,"mode":null,"firstInput":null}
    }))
    .unwrap();
    assert_eq!(
        BoardDeliveryTypes::max_acceptance_bytes_for(&body),
        64 * 1024
    );
    let result: BoardDeliveryBody = serde_json::from_value(json!({
        "kind":"remoteCreateResult","requestId":ID,"outcome":{"kind":"expired"}
    }))
    .unwrap();
    assert_eq!(BoardDeliveryTypes::max_acceptance_bytes_for(&result), 64);
}

#[test]
fn retained_d1_address_bytes_round_trip_without_a_wrapper() {
    for bytes in [
        SESSION_BYTES,
        r#"{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},"sessionId":"quotes\" slash\\ unicode 🐒"}"#,
    ] {
        let address: <BoardDeliveryTypes as DeliveryTypes>::Address =
            serde_json::from_str(bytes).unwrap();
        assert!(matches!(address, DeliveryAddress::Session(_)));
        assert_eq!(serde_json::to_string(&address).unwrap(), bytes);
    }
}

#[test]
fn machine_address_is_an_additive_closed_wire_shape() {
    let address: <BoardDeliveryTypes as DeliveryTypes>::Address =
        serde_json::from_value(json!({"machine":ID})).unwrap();
    assert_eq!(
        serde_json::to_value(&address).unwrap(),
        json!({"machine":ID})
    );
    let mut both: serde_json::Value = serde_json::from_str(SESSION_BYTES).unwrap();
    both["machine"] = json!(ID);
    assert!(
        serde_json::from_value::<<BoardDeliveryTypes as DeliveryTypes>::Address>(both).is_err()
    );
}

#[test]
fn every_sender_body_address_combination_obeys_the_program_design() {
    let session: serde_json::Value = serde_json::from_str(SESSION_BYTES).unwrap();
    let mut session_sender = session.clone();
    session_sender["kind"] = json!("session");
    let senders = [
        session_sender.clone(),
        json!({"kind":"humanUser","installation":ID}),
        json!({"kind":"project","project":ID}),
        json!({"kind":"fleet"}),
    ];
    let bodies = [
        json!({"kind":"message","text":"hello","delivery":"queue","guard":null,"senderDisplayName":null,"replyTo":null}),
        json!({"kind":"notification","excerpt":"notice","pointer":{"project":ID,"subject":{"kind":"task","id":ID},"position":{"epoch":1,"position":1}},"source":{"kind":"intent","intent":ID}}),
        json!({"kind":"wake","request":{"kind":"show","wakeupId":ID},"requestedBy":session_sender}),
        json!({"kind":"wake","request":{"kind":"show","wakeupId":ID},"requestedBy":{"kind":"owner"}}),
        json!({"kind":"createSession","requestId":ID,"requester":session,"command":{"endpoint":"codex-local","workingDirectory":"/tmp","access":"workspace-write","model":null,"effort":null,"mode":null,"firstInput":null}}),
        json!({"kind":"remoteCreateResult","requestId":ID,"outcome":{"kind":"expired"}}),
    ];
    // Oracle: PD Delivery address row, intersected with D1's body sender rules.
    // Columns: message, notification, session wake, owner wake, create, result.
    let session_allowed = [
        [true, false, true, false, false, false],
        [true, false, false, true, false, false],
        [false, true, false, false, false, false],
        [false, false, false, false, false, true],
    ];
    for (sender_index, sender) in senders.into_iter().enumerate() {
        let sender: MailSender = serde_json::from_value(sender).unwrap();
        for (body_index, body) in bodies.iter().enumerate() {
            let body: BoardDeliveryBody = serde_json::from_value(body.clone()).unwrap();
            for (machine, address) in [
                DeliveryAddress::Session(serde_json::from_value(session.clone()).unwrap()),
                DeliveryAddress::Machine {
                    machine: ID.parse().unwrap(),
                },
            ]
            .into_iter()
            .enumerate()
            {
                let expected = if machine == 0 {
                    session_allowed[sender_index][body_index]
                } else {
                    sender_index == 3 && body_index == 4
                };
                assert_eq!(
                    validate_delivery_address(&sender, &body, &address).is_ok(),
                    expected,
                    "sender {sender_index}, body {body_index}, machine {machine}"
                );
            }
        }
    }
}

#[test]
fn no_address_decodes_as_both_closed_shapes() {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct MachineFields {
        machine: fleet_sync_protocol::PeerId,
    }
    let session: serde_json::Value = serde_json::from_str(SESSION_BYTES).unwrap();
    let mut both = session.clone();
    both["machine"] = json!(ID);
    for fixture in [
        session,
        json!({"machine":ID}),
        both,
        json!({}),
        json!({"machine":ID,"extra":true}),
        json!({"machine":null}),
    ] {
        let session_shape =
            serde_json::from_value::<message_board::SessionRef>(fixture.clone()).is_ok();
        let machine_shape = serde_json::from_value::<MachineFields>(fixture.clone())
            .map(|fields| fields.machine)
            .is_ok();
        assert!(!(session_shape && machine_shape), "{fixture}");
        assert_eq!(
            serde_json::from_value::<DeliveryAddress>(fixture.clone()).is_ok(),
            session_shape || machine_shape,
            "{fixture}"
        );
    }
}

#[test]
fn ordinary_mail_cannot_submit_a_machine_address_through_the_wire_decoder() {
    let body: BoardDeliveryBody = serde_json::from_value(json!({"kind":"message","text":"hello",
        "delivery":"queue","guard":null,"senderDisplayName":null,"replyTo":null}))
    .unwrap();
    let item = fleet_sync_protocol::DeliveryItem::<BoardDeliveryTypes>::new(
        ID.to_owned().try_into().unwrap(),
        MailSender::HumanUser {
            installation: ID.to_owned().try_into().unwrap(),
        },
        fleet_sync_protocol::Recipient {
            peer: ID.parse().unwrap(),
            address: DeliveryAddress::Machine {
                machine: ID.parse().unwrap(),
            },
        },
        None,
        body,
    )
    .unwrap();
    let request = json!({"kind":"mailSubmit","value":{"item":fleet_sync_protocol::DeliveryEnvelope::Item(item),"originEpoch":null}});
    let error = serde_json::from_value::<crate::BoardConsumerRequest>(request)
        .err()
        .unwrap();
    assert!(error.to_string().contains("recipient.address"));
}
