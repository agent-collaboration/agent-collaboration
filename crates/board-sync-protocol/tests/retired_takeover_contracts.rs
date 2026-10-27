use board_sync_protocol::BoardConsumerProtocol;
use fleet_sync_protocol as wire;
use serde_json::{Value, json};

const FRAME: &str = "019a1234-5678-7000-8000-000000000001";
const PROJECT: &str = "019a1234-5678-7000-8000-000000000002";

fn request(body: Value) -> Value {
    json!({"kind":"request","value":{"frame":FRAME,"body":{"kind":"consumer","value":body}}})
}

#[test]
fn library_frames_refuse_removed_tail_reads_and_report_answers() {
    for scope in [
        json!({"kind":"tailReports","value":{"project":PROJECT}}),
        json!({"kind":"tailLineage","value":{"project":PROJECT,"observedEpoch":1,"currentEpoch":2}}),
    ] {
        assert!(
            serde_json::from_value::<wire::ClientFrame<BoardConsumerProtocol>>(request(
                json!({"kind":"read","value":{"scope":scope,"cursor":null}})
            ))
            .is_err()
        );
    }
    let answer = json!({"kind":"answer","value":{"frame":FRAME,"body":{"kind":"consumer","value":{
        "kind":"rejectedTailReport","value":{"kind":"notEnded"}
    }}}});
    assert!(serde_json::from_value::<wire::ServerFrame<BoardConsumerProtocol>>(answer).is_err());
    let current =
        request(json!({"kind":"read","value":{"scope":{"kind":"fleetRecords"},"cursor":null}}));
    let decoded: wire::ClientFrame<BoardConsumerProtocol> =
        serde_json::from_value(current.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), current);
}
