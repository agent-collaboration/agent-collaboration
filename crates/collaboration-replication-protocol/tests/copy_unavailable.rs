#![allow(clippy::expect_used)]

use collaboration_replication_protocol::{MoveResult, SessionAnswer, TakeoverResult};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

const PROJECT: &str = "019f0000-0000-7000-8000-000000000001";
const TRANSFER: &str = "019f0000-0000-7000-8000-000000000002";

fn round_trip<TValue: DeserializeOwned + Serialize>(wire: Value) {
    let decoded = serde_json::from_value::<TValue>(wire.clone()).expect("accepted contract");
    assert_eq!(serde_json::to_value(decoded).expect("encode"), wire);
}

#[test]
fn shared_session_unavailability_carries_a_closed_project_or_transfer_subject() {
    for subject in [
        json!({"kind":"project","project":PROJECT}),
        json!({"kind":"transfer","transfer":TRANSFER}),
    ] {
        for reason in [
            json!({"kind":"storageFull"}),
            json!({"kind":"migrationPending","value":{"kind":"copy","version":3}}),
        ] {
            round_trip::<SessionAnswer>(json!({"kind":"unavailable","value":{
                "subject":subject,"reason":reason
            }}));
        }
    }
    for subject in [
        json!({"kind":"unknown","project":PROJECT}),
        json!({"kind":"project","project":PROJECT,"transfer":TRANSFER}),
        json!({"kind":"transfer","project":PROJECT}),
        json!({"kind":"project","project":"invalid"}),
        json!({"project":PROJECT}),
        Value::Null,
    ] {
        assert!(
            serde_json::from_value::<SessionAnswer>(json!({
                "kind":"unavailable","value":{"subject":subject,"reason":{"kind":"storageFull"}}
            }))
            .is_err(),
            "reject invalid subject {subject}"
        );
    }
}

#[test]
fn transfer_only_results_keep_the_existing_transfer_identity() {
    let wire = json!({"kind":"unavailable","transfer":TRANSFER,"reason":{"kind":"storageFull"}});
    round_trip::<MoveResult>(wire.clone());
    round_trip::<TakeoverResult>(wire);
    for wire in [
        json!({"kind":"unavailable","project":PROJECT,"reason":{"kind":"storageFull"}}),
        json!({"kind":"unavailable","transfer":TRANSFER,"project":PROJECT,"reason":{"kind":"storageFull"}}),
    ] {
        assert!(serde_json::from_value::<MoveResult>(wire.clone()).is_err());
        assert!(serde_json::from_value::<TakeoverResult>(wire).is_err());
    }
}
