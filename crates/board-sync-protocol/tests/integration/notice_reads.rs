//! Real public frames preserve control progress and reject retired notice contracts.
use board_sync_protocol::BoardConsumerProtocol;
use fleet_protocol as wire;
use serde_json::{Value, json};

const FRAME: &str = "019a1234-5678-7000-8000-000000000002";
const AS_OF: &str = "2026-10-19T11:00:00Z";

// Frozen consumer notice contract from cfd525e16:board_payloads.rs:96-119.
// Test-only: it records the deliberately unsupported old client, not a shim.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum FrozenOldNotice {
    FleetFeedAdvanced {
        through: message_board::FleetPosition,
    },
    LeaseSuperseded {
        project: message_board::ProjectId,
        epoch: wire::Epoch,
    },
    LeaseGranted {
        project: message_board::ProjectId,
        epoch: wire::Epoch,
    },
    BoardArchiving {
        board: message_board::BoardId,
    },
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FrozenOldPositionedNotice {
    position: wire::NoticePosition,
    kind: FrozenOldNotice,
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum FrozenOldNoticeRead {
    Page {
        notices: Vec<FrozenOldPositionedNotice>,
        as_of: chrono::DateTime<chrono::Utc>,
    },
    CursorExpired {
        floor: wire::NoticePosition,
    },
    CursorInvalid,
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
enum FrozenOldReadPage {
    Notices(FrozenOldNoticeRead),
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
enum FrozenOldAnswer {
    Read(FrozenOldReadPage),
}
struct FrozenOldProtocol;
impl wire::ConsumerProtocol for FrozenOldProtocol {
    type Delivery = <BoardConsumerProtocol as wire::ConsumerProtocol>::Delivery;
    type Request = <BoardConsumerProtocol as wire::ConsumerProtocol>::Request;
    type Answer = FrozenOldAnswer;
    type ServerRequest = <BoardConsumerProtocol as wire::ConsumerProtocol>::ServerRequest;
    type ClientAnswer = <BoardConsumerProtocol as wire::ConsumerProtocol>::ClientAnswer;
    type Notice = FrozenOldPositionedNotice;
    type Annotation = <BoardConsumerProtocol as wire::ConsumerProtocol>::Annotation;
}

#[test]
fn frozen_old_push_decoder_rejects_d1_project_change_but_shared_lease_notices_still_decode() {
    let notice = json!({"position":1,"kind":{"kind":"projectListChanged","value":{}}});
    for changed in [
        pushed_notice(notice.clone()),
        read_answer(json!({"kind":"page","value":{"notices":[notice],"asOf":AS_OF}})),
    ] {
        assert!(
            serde_json::from_value::<wire::ServerFrame<BoardConsumerProtocol>>(changed.clone())
                .is_ok()
        );
        let old_error = serde_json::from_value::<wire::ServerFrame<FrozenOldProtocol>>(changed)
            .err()
            .expect("old decoder must reject D1's new notice variant");
        assert!(
            old_error
                .to_string()
                .contains("unknown variant `projectListChanged`"),
            "wrong failure: {old_error}"
        );
        eprintln!("PROTOCOL_NEGATIVE_CONTROL old_decoder_rejects_D1_notice: {old_error}");
    }
    let shared = pushed_notice(
        json!({"position":1,"kind":{"kind":"leaseGranted","value":{"project":FRAME,"epoch":1}}}),
    );
    assert!(serde_json::from_value::<wire::ServerFrame<FrozenOldProtocol>>(shared.clone()).is_ok());
    assert!(serde_json::from_value::<wire::ServerFrame<BoardConsumerProtocol>>(shared).is_ok());
}

#[test]
fn d1_decoder_rejects_frozen_old_fleet_hints_on_both_push_and_catch_up() {
    for kind in [
        json!({"kind":"fleetFeedAdvanced","value":{"through":1}}),
        json!({"kind":"boardArchiving","value":{"board":FRAME}}),
    ] {
        let notice = json!({"position":1,"kind":kind});
        let pushed = pushed_notice(notice.clone());
        assert!(
            serde_json::from_value::<wire::ServerFrame<FrozenOldProtocol>>(pushed.clone()).is_ok()
        );
        for frame in [
            pushed,
            read_answer(json!({"kind":"page","value":{"notices":[notice],"asOf":AS_OF}})),
        ] {
            assert!(
                serde_json::from_value::<wire::ServerFrame<BoardConsumerProtocol>>(frame).is_err()
            );
        }
    }
}

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
        json!({"kind":"page","value":{"notices":[{"position":73,"kind":{"kind":"projectListChanged","value":{}}}],"asOf":AS_OF}}),
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
    let expected =
        pushed_notice(json!({"position":73,"kind":{"kind":"projectListChanged","value":{}}}));
    let decoded: wire::ServerFrame<BoardConsumerProtocol> =
        serde_json::from_value(expected.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), expected);
}

#[test]
fn three_control_notice_kinds_roundtrip_on_push_and_catch_up() {
    for kind in [
        json!({"kind":"projectListChanged","value":{}}),
        json!({"kind":"leaseGranted","value":{"project":FRAME,"epoch":7}}),
        json!({"kind":"leaseSuperseded","value":{"project":FRAME,"epoch":8}}),
    ] {
        let notice = json!({"position":73,"kind":kind});
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
fn removed_mail_waiting_notices_are_rejected_on_push_and_catch_up() {
    for fields in [
        json!({"stream":"items"}),
        json!({"stream":"controls"}),
        json!({}),
        json!({"stream":"invented"}),
        json!({"stream":0}),
        json!({"stream":"items","through":73}),
    ] {
        let notice = json!({"position":73,"kind":{"kind":"mailWaiting","value":fields}});
        for frame in [
            pushed_notice(notice.clone()),
            read_answer(json!({"kind":"page","value":{"notices":[notice],"asOf":AS_OF}})),
        ] {
            assert!(
                serde_json::from_value::<wire::ServerFrame<BoardConsumerProtocol>>(frame).is_err()
            );
        }
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
        json!({"kind":"projectListChanged","value":{}}),
        json!({"kind":{"kind":"projectListChanged","value":{}}}),
        json!({"position":73,"kind":{"kind":"projectListChanged","value":{}},"extra":true}),
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
