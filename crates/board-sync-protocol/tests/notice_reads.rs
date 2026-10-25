#![allow(clippy::unwrap_used)]
//! The shared frame codec distinguishes transport progress, machine progress and read refusals.
use board_sync_protocol::BoardConsumerProtocol;
use fleet_sync_protocol as wire;
use serde_json::{Value, json};

const FRAME: &str = "019a1234-5678-7000-8000-000000000002";
const AS_OF: &str = "2026-10-19T11:00:00Z";

fn read_answer(result: Value) -> Value {
    json!({"kind":"answer","value":{"frame":FRAME,"body":{"kind":"consumer","value":{
        "kind":"read","value":{"kind":"notices","value":result}
    }}}})
}

fn pushed_notice(consumer: Value) -> Value {
    json!({"kind":"notice","value":{"position":3,"body":{"kind":"consumer","value":consumer}}})
}

#[test]
fn notice_reads_preserve_pages_and_distinct_expired_and_invalid_answers() {
    for result in [
        json!({"kind":"page","value":{"notices":[{"position":73,"kind":{"kind":"fleetFeedAdvanced","value":{"through":91}}}],"asOf":AS_OF}}),
        json!({"kind":"page","value":{"notices":[],"asOf":AS_OF}}),
        json!({"kind":"cursorExpired","value":{"floor":70}}),
        json!({"kind":"cursorInvalid"}),
    ] {
        let expected = read_answer(result);
        let decoded: wire::ServerFrame<BoardConsumerProtocol> =
            serde_json::from_value(expected.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), expected);
    }
}

#[test]
fn pushed_consumer_notice_keeps_machine_position_separate_from_transport_position() {
    let expected = pushed_notice(
        json!({"position":73,"kind":{"kind":"fleetFeedAdvanced","value":{"through":91}}}),
    );
    let decoded: wire::ServerFrame<BoardConsumerProtocol> =
        serde_json::from_value(expected.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), expected);
}

#[test]
fn mail_waiting_notice_preserves_each_library_stream_on_push_and_catch_up() {
    // Decision 670 names a stream hint, not a mail sequence or an item payload.
    // The real frame codec must accept the producer's literal closed shape.
    for stream in ["items", "controls"] {
        let notice = json!({"position":73,"kind":{"kind":"mailWaiting","value":{"stream":stream}}});
        for expected in [
            pushed_notice(notice.clone()),
            read_answer(json!({"kind":"page","value":{"notices":[notice],"asOf":AS_OF}})),
        ] {
            let decoded: wire::ServerFrame<BoardConsumerProtocol> =
                serde_json::from_value(expected.clone()).unwrap();
            assert_eq!(serde_json::to_value(decoded).unwrap(), expected);
        }
    }
}

#[test]
fn mail_waiting_notice_rejects_missing_unknown_or_extra_stream_fields() {
    for fields in [
        json!({}),
        json!({"stream":"invented"}),
        json!({"stream":0}),
        json!({"stream":"items","through":73}),
    ] {
        let notice = json!({"position":73,"kind":{"kind":"mailWaiting","value":fields}});
        assert!(
            serde_json::from_value::<wire::ServerFrame<BoardConsumerProtocol>>(pushed_notice(
                notice.clone()
            ))
            .is_err(),
            "accepted invalid mail-waiting notice: {notice}"
        );
    }
}

#[test]
fn the_removed_release_hint_is_not_a_consumer_notice() {
    // Release is the existing SDK ServerCall::Release, never a second hint contract.
    let removed = json!({"position":73,"kind":{"kind":"releaseRequested","value":{
        "project":FRAME,"epoch":7,"transfer":FRAME
    }}});
    for expected in [
        pushed_notice(removed.clone()),
        read_answer(json!({"kind":"page","value":{"notices":[removed],"asOf":AS_OF}})),
    ] {
        assert!(
            serde_json::from_value::<wire::ServerFrame<BoardConsumerProtocol>>(expected).is_err()
        );
    }
}

#[test]
fn notice_read_refuses_the_removed_flat_shape_and_incomplete_or_unknown_answers() {
    for result in [
        json!({"notices":[],"asOf":AS_OF}),
        json!({"kind":"page","value":{"notices":[]}}),
        json!({"kind":"page","value":{"notices":[],"asOf":AS_OF,"extra":true}}),
        json!({"kind":"cursorExpired","value":{}}),
        json!({"kind":"cursorExpired","value":{"floor":70,"head":73}}),
        json!({"kind":"cursorInvalid","value":{"floor":70}}),
        json!({"kind":"invented"}),
    ] {
        assert!(
            serde_json::from_value::<wire::ServerFrame<BoardConsumerProtocol>>(read_answer(
                result.clone()
            ))
            .is_err(),
            "accepted invalid notice read: {result}"
        );
    }
}

#[test]
fn consumer_notice_refuses_a_missing_machine_position_or_unknown_fields() {
    for consumer in [
        json!({"kind":"fleetFeedAdvanced","value":{"through":91}}),
        json!({"kind":{"kind":"fleetFeedAdvanced","value":{"through":91}}}),
        json!({"position":73,"kind":{"kind":"fleetFeedAdvanced","value":{"through":91}},"extra":true}),
    ] {
        assert!(
            serde_json::from_value::<wire::ServerFrame<BoardConsumerProtocol>>(pushed_notice(
                consumer.clone()
            ))
            .is_err(),
            "accepted invalid consumer notice: {consumer}"
        );
    }
}

#[test]
fn notice_read_requests_keep_optional_machine_cursor() {
    for cursor in [json!(null), json!({"kind":"notices","value":73})] {
        let expected = json!({"kind":"request","value":{"frame":FRAME,"body":{"kind":"consumer","value":{
            "kind":"read","value":{"scope":{"kind":"notices"},"cursor":cursor}
        }}}});
        let decoded: wire::ClientFrame<BoardConsumerProtocol> =
            serde_json::from_value(expected.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), expected);
    }
}
