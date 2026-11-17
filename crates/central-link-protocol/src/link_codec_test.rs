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
            json!({"kind":"deliver","message":{"messageId":id,"sender":{"kind":"central"},"target":{"machineId":id,"endpoint":"codex","sessionId":"s"},"body":{"kind":"direct","body":"hello","delivery":"auto","replyTo":null},"state":{"kind":"queued"},"source":null,"createdAt":"2030-01-01T00:00:00Z","updatedAt":"2030-01-01T00:00:00Z"}}),
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
