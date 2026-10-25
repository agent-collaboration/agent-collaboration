#![allow(clippy::unwrap_used)]

use board_sync_protocol::BoardConsumerProtocol;
use fleet_sync_protocol as wire;
use serde_json::{Value, json};

const FRAME: &str = "019a1234-5678-7000-8000-000000000001";
const RECORD: &str = "019a1234-5678-7000-8000-000000000002";
const AS_OF: &str = "2026-10-19T11:00:00Z";

fn record_ref() -> Value {
    json!({"kind":"workspace","id":RECORD})
}

fn read_request(scope: Value, cursor: Value) -> Value {
    json!({"kind":"request","value":{"frame":FRAME,"body":{"kind":"consumer","value":{
        "kind":"read","value":{"scope":scope,"cursor":cursor}
    }}}})
}

fn read_answer(page: Value) -> Value {
    json!({"kind":"answer","value":{"frame":FRAME,"body":{"kind":"consumer","value":{
        "kind":"read","value":page
    }}}})
}

#[test]
fn incremental_fleet_reads_keep_the_existing_scope_and_position_cursor() {
    for cursor in [json!(null), json!({"kind":"fleet","value":41})] {
        let expected = read_request(json!({"kind":"fleetRecords"}), cursor);
        let decoded: wire::ClientFrame<BoardConsumerProtocol> =
            serde_json::from_value(expected.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), expected);
    }
}

#[test]
fn fleet_feed_answers_preserve_page_progress_and_distinct_cursor_refusals() {
    for result in [
        json!({"kind":"page","value":{"records":[{"recordId":RECORD}],"through":41,"head":57,"asOf":AS_OF}}),
        json!({"kind":"page","value":{"records":[],"through":57,"head":57,"asOf":AS_OF}}),
        json!({"kind":"cursorExpired","value":{"floor":32}}),
        json!({"kind":"cursorInvalid"}),
    ] {
        let expected = read_answer(json!({"kind":"fleetFeed","value":result}));
        let decoded: wire::ServerFrame<BoardConsumerProtocol> =
            serde_json::from_value(expected.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), expected);
    }
}

#[test]
fn full_fleet_reads_carry_the_fixed_snapshot_and_record_identity_on_continuation() {
    for cursor in [
        json!(null),
        json!({"kind":"fleetSnapshot","value":{"snapshotAt":57,"after":record_ref()}}),
    ] {
        let expected = read_request(json!({"kind":"fleetSnapshot"}), cursor);
        let decoded: wire::ClientFrame<BoardConsumerProtocol> =
            serde_json::from_value(expected.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), expected);
    }
}

#[test]
fn full_fleet_pages_keep_the_snapshot_cut_and_explicit_completion() {
    for (records, next) in [
        (
            json!([{"recordId":RECORD}]),
            json!({"snapshotAt":57,"after":record_ref()}),
        ),
        (json!([]), json!(null)),
    ] {
        let expected = read_answer(json!({"kind":"fleetSnapshot","value":{
            "records":records,"next":next,"snapshotAt":57,"asOf":AS_OF
        }}));
        let decoded: wire::ServerFrame<BoardConsumerProtocol> =
            serde_json::from_value(expected.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), expected);
    }
}

#[test]
fn fleet_feed_answers_reject_removed_ambiguous_and_unknown_shapes() {
    let mut invalid_pages = vec![json!({"kind":"fleetRecords","value":{
        "records":[],"position":41,"asOf":AS_OF
    }})];
    invalid_pages.extend([
        json!({"kind":"page","value":{"records":[],"through":41,"asOf":AS_OF}}),
        json!({"kind":"page","value":{"records":[],"position":41,"head":57,"asOf":AS_OF}}),
        json!({"kind":"page","value":{"records":[],"through":41,"head":57,"asOf":AS_OF,"extra":true}}),
        json!({"kind":"cursorExpired","value":{}}),
        json!({"kind":"cursorExpired","value":{"floor":32,"head":57}}),
        json!({"kind":"cursorInvalid","value":{"floor":32}}),
        json!({"kind":"invented"}),
    ].map(|result| json!({"kind":"fleetFeed","value":result})));
    for invalid in invalid_pages {
        assert!(
            serde_json::from_value::<wire::ServerFrame<BoardConsumerProtocol>>(read_answer(
                invalid.clone()
            ))
            .is_err(),
            "accepted invalid fleet page: {invalid}"
        );
    }
}

#[test]
fn full_fleet_reads_reject_missing_invalid_or_unknown_cursor_fields() {
    for cursor in [
        json!({"after":record_ref()}),
        json!({"snapshotAt":57}),
        json!({"snapshotAt":57,"after":"not-a-record"}),
        json!({"snapshotAt":57,"after":record_ref(),"storageKey":1}),
        json!({"snapshot_at":57,"after":record_ref()}),
    ] {
        let invalid = read_request(
            json!({"kind":"fleetSnapshot"}),
            json!({"kind":"fleetSnapshot","value":cursor}),
        );
        assert!(
            serde_json::from_value::<wire::ClientFrame<BoardConsumerProtocol>>(invalid).is_err()
        );
    }
    for page in [
        json!({"records":[],"next":null,"asOf":AS_OF}),
        json!({"records":[],"next":null,"snapshotAt":57,"asOf":AS_OF,"extra":true}),
        json!({"records":[],"next":{"snapshotAt":57,"after":record_ref(),"extra":true},"snapshotAt":57,"asOf":AS_OF}),
    ] {
        let invalid = read_answer(json!({"kind":"fleetSnapshot","value":page}));
        assert!(
            serde_json::from_value::<wire::ServerFrame<BoardConsumerProtocol>>(invalid).is_err()
        );
    }
}
