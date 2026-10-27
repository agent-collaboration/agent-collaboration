use board_sync_protocol::BoardConsumerProtocol;
use fleet_sync_protocol as wire;
use serde_json::{Value, json};
const FRAME: &str = "019a1234-5678-7000-8000-000000000001";

fn answer(value: Value) -> Value {
    json!({"kind":"answer","value":{"frame":FRAME,"body":{"kind":"consumer","value":{
        "kind":"mailSubmit","value":value
    }}}})
}

#[test]
fn mail_submit_answers_round_trip_library_results_and_distinct_retryable_outcomes() {
    for result in [
        json!({"kind":"submitted","value":{"kind":"accepted","value":{"seq":11,"status":{"kind":"sent","value":{"seq":11}},"version":1,"cancel":null}}}),
        json!({"kind":"submitted","value":{"kind":"accepted","value":{"seq":11,"status":{"kind":"expired"},"version":2,"cancel":null}}}),
        json!({"kind":"submitted","value":{"kind":"replayed","value":{"status":{"kind":"delivered","value":{"acceptance":"peerMessageWritten"}},"version":u64::MAX,"cancel":{"kind":"tooLate","value":{"at":"delivered"}}}}}),
        json!({"kind":"submitted","value":{"kind":"refused","value":{"reason":{"kind":"itemConflict","value":{"existing":"a".repeat(64)}}}}}),
        json!({"kind":"mailboxUnavailable","value":{"reason":{"kind":"processingFailed","value":{"kind":"custodyCorrupt"}}}}),
        json!({"kind":"notWriter"}),
    ] {
        let expected = answer(result);
        let decoded =
            serde_json::from_value::<wire::ServerFrame<BoardConsumerProtocol>>(expected.clone());
        assert!(
            decoded.is_ok(),
            "missing board submit answer: {}",
            decoded
                .err()
                .map(|error| error.to_string())
                .unwrap_or_default()
        );
        assert_eq!(serde_json::to_value(decoded.unwrap()).unwrap(), expected);
    }
}

#[test]
fn mail_submit_answers_reject_unknown_variants_and_fields() {
    for invalid in [
        json!({"kind":"invented"}),
        json!({"kind":"storageFailure"}),
        json!({"kind":"invalidShape","value":{"field":"originEpoch"}}),
        json!({"kind":"notWriter","current":7}),
        json!({"kind":"notWriter","value":{"current":7}}),
        json!({"kind":"mailboxUnavailable","value":{"reason":{"kind":"mailboxFull"},"extra":true}}),
        json!({"kind":"submitted","value":{"kind":"refused","value":{"reason":{"kind":"invented"}}}}),
        json!({"kind":"submitted","value":{"kind":"accepted","value":{"seq":11,"status":{"kind":"queued"},"version":1,"cancel":null,"extra":true}}}),
    ] {
        assert!(
            serde_json::from_value::<wire::ServerFrame<BoardConsumerProtocol>>(answer(
                invalid.clone()
            ))
            .is_err(),
            "accepted invalid submit answer: {invalid}"
        );
    }
}

#[test]
fn invalid_shape_remains_the_library_session_answer() {
    let expected = json!({"kind":"answer","value":{"frame":FRAME,"body":{
        "kind":"invalidShape","value":{"field":"originEpoch"}
    }}});
    let decoded: wire::ServerFrame<BoardConsumerProtocol> =
        serde_json::from_value(expected.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), expected);
}
