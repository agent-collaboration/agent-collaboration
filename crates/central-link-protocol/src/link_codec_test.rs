use super::*;
use serde_json::json;

#[test]
fn heartbeat_is_exact_unquoted_text_and_old_nonce_json_is_rejected() {
    assert_eq!(
        encode_text(&LinkFrame::Ping, FrameDirection::ToCentral).unwrap(),
        "ping"
    );
    assert_eq!(
        encode_text(&LinkFrame::Pong, FrameDirection::ToRouter).unwrap(),
        "pong"
    );
    assert_eq!(
        decode_text("ping", FrameDirection::ToCentral).unwrap(),
        LinkFrame::Ping
    );
    assert_eq!(
        decode_text("pong", FrameDirection::ToRouter).unwrap(),
        LinkFrame::Pong
    );
    for wire in [
        "\"ping\"",
        " ping",
        "ping\n",
        "PING",
        r#"{"kind":"ping","nonce":1}"#,
        r#"{"kind":"pong"}"#,
    ] {
        assert!(
            decode_text(wire, FrameDirection::ToCentral).is_err(),
            "{wire}"
        );
    }
    assert!(decode_text("pong", FrameDirection::ToCentral).is_err());
    assert!(encode_text(&LinkFrame::Ping, FrameDirection::ToRouter).is_err());
}

#[test]
fn welcome_reply_deliver_and_ack_are_closed_and_directional() {
    let id = "01890f2e-7b4c-7cc0-98c4-000000000001";
    let fixtures = [
        (
            FrameDirection::ToRouter,
            json!({"kind":"welcome","machineId":id,"centralTime":"2030-01-01T00:00:00Z"}),
        ),
        (
            FrameDirection::ToRouter,
            json!({"kind":"reply","correlationId":id,"result":{"kind":"workspace","reply":{"result":{"kind":"centralUnavailable"},"actors":[]}}}),
        ),
        (
            FrameDirection::ToRouter,
            json!({"kind":"deliver","message":{"messageId":id,"sender":{"kind":"central"},"target":{"machineId":id,"endpoint":"codex","sessionId":"s"},"body":{"kind":"direct","body":"hello"},"state":{"kind":"queued"},"source":null,"createdAt":"2030-01-01T00:00:00Z","updatedAt":"2030-01-01T00:00:00Z"}}),
        ),
        (
            FrameDirection::ToCentral,
            json!({"kind":"ack","messageId":id}),
        ),
    ];
    for (direction, fixture) in fixtures {
        let encoded = fixture.to_string();
        let frame = decode_text(&encoded, direction).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&encode_text(&frame, direction).unwrap())
                .unwrap(),
            fixture
        );
        let other = match direction {
            FrameDirection::ToCentral => FrameDirection::ToRouter,
            FrameDirection::ToRouter => FrameDirection::ToCentral,
        };
        assert!(decode_text(&encoded, other).is_err());
        let mut bad = fixture;
        bad["extra"] = json!(true);
        assert!(decode_text(&bad.to_string(), direction).is_err());
    }
    for fixture in [
        json!({"kind":"unknown"}),
        json!({"kind":"ack"}),
        json!({"kind":"ack","messageId":"bad"}),
    ] {
        assert!(decode_text(&fixture.to_string(), FrameDirection::ToCentral).is_err());
    }
}

#[test]
fn mail_request_and_reply_reuse_the_existing_contract_without_wire_coercion() {
    let id = "01890f2e-7b4c-7cc0-98c4-000000000001";
    let fixtures = [
        (
            FrameDirection::ToCentral,
            json!({"kind":"request","request":{"correlationId":id,"caller":{"kind":"session","value":{"machineId":id,"endpoint":"codex","sessionId":"s"}},"payload":{"kind":"mail","request":{"requestId":"01890f2e-7b4c-7cc0-98c4-000000000002","operation":{"kind":"setNoticeCadence","cadence":"m25"}}}}}),
        ),
        (
            FrameDirection::ToRouter,
            json!({"kind":"reply","correlationId":id,"result":{"kind":"mail","reply":{"kind":"centralUnavailable"}}}),
        ),
    ];
    for (direction, fixture) in fixtures {
        let parsed = decode_text(&fixture.to_string(), direction).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&encode_text(&parsed, direction).unwrap())
                .unwrap(),
            fixture
        );
    }
}

#[test]
fn batch_of_writes_round_trips_raw_json() {
    let fixture = json!({"kind":"batch","requests":[{"correlationId":"01890f2e-7b4c-7cc0-98c4-000000000001","caller":{"kind":"session","value":{"machineId":"01890f2e-7b4c-7cc0-98c4-000000000002","endpoint":"codex","sessionId":"s"}},"payload":{"kind":"workspace","request":{"workspaceId":"01890f2e-7b4c-7cc0-98c4-000000000003","requestId":"01890f2e-7b4c-7cc0-98c4-000000000004","operation":{"kind":"createArea","name":"Work"}}}}]});
    let frame = decode_text(&fixture.to_string(), FrameDirection::ToCentral).unwrap();
    let encoded = encode_text(&frame, FrameDirection::ToCentral).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&encoded).unwrap(),
        fixture
    );
}

fn batch_member(number: usize) -> serde_json::Value {
    json!({"correlationId":format!("01890f2e-7b4c-7cc0-98c4-{number:012x}"),"caller":{"kind":"session","value":{"machineId":"01890f2e-7b4c-7cc0-98c4-000000000002","endpoint":"codex","sessionId":"s"}},"payload":{"kind":"workspace","request":{"workspaceId":"01890f2e-7b4c-7cc0-98c4-000000000003","requestId":"01890f2e-7b4c-7cc0-98c4-000000000004","operation":{"kind":"createArea","name":"Work"}}}})
}

#[test]
fn batch_enforces_bound_and_unique_correlations_for_wire_and_constructor() {
    let members: Vec<_> = (0..64).map(batch_member).collect();
    let wire = json!({"kind":"batch", "requests":members});
    let frame = decode_text(&wire.to_string(), FrameDirection::ToCentral).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(
            &encode_text(&frame, FrameDirection::ToCentral).unwrap()
        )
        .unwrap(),
        wire
    );
    let LinkFrame::Data(DataFrame::Batch { requests }) = frame else {
        panic!("expected batch")
    };
    assert_eq!(requests.requests().len(), 64);
    let mut too_many = requests.into_requests();
    too_many.push(too_many[0].clone());
    assert!(crate::LinkRequestBatch::new(too_many).is_err());
    for requests in [
        vec![],
        (0..65).map(batch_member).collect(),
        vec![batch_member(1), batch_member(1)],
    ] {
        assert!(
            decode_text(
                &json!({"kind":"batch", "requests":requests}).to_string(),
                FrameDirection::ToCentral
            )
            .is_err()
        );
    }
}

#[test]
fn batch_rejects_reads_wrong_direction_unknown_fields_and_malformed_members() {
    let mut read = batch_member(1);
    read["payload"]["request"]["requestId"] = serde_json::Value::Null;
    read["payload"]["request"]["operation"] =
        serde_json::to_value(workspace_model::WorkspaceOperation::List {
            query: workspace_model::ListQuery::Areas,
            after: 0_u64.try_into().unwrap(),
            limit: workspace_model::PageLimit::try_from(10).unwrap(),
        })
        .unwrap();
    let read_wire = json!({"kind":"batch","requests":[read]});
    assert!(decode_text(&read_wire.to_string(), FrameDirection::ToCentral).is_err());
    let valid = json!({"kind":"batch","requests":[batch_member(1)]});
    assert!(matches!(
        decode_text(&valid.to_string(), FrameDirection::ToRouter),
        Err(ProtocolError::WrongDirection)
    ));
    let frame = decode_text(&valid.to_string(), FrameDirection::ToCentral).unwrap();
    assert!(matches!(
        encode_text(&frame, FrameDirection::ToRouter),
        Err(ProtocolError::WrongDirection)
    ));
    let mut extra = valid.clone();
    extra["extra"] = json!(true);
    let mut member_extra = valid.clone();
    member_extra["requests"][0]["extra"] = json!(true);
    let mut invalid = valid;
    invalid["requests"][0]["payload"]["request"]["requestId"] = json!("bad");
    for wire in [
        extra,
        member_extra,
        invalid,
        json!({"kind":"batch", "requests":[null]}),
    ] {
        assert!(decode_text(&wire.to_string(), FrameDirection::ToCentral).is_err());
    }
}
