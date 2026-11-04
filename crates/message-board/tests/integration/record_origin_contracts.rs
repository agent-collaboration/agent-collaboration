use message_board::RecordPlacement;
use serde_json::json;

#[test]
fn sequenced_positions_preserve_their_required_source_origin() {
    let origin =
        json!({"installation":"00000000-0000-4000-8000-000000000001","machine":"source machine"});
    for encoded in [
        json!({"kind":"project","position":{"cursor":{"epoch":1,"position":7},"origin":origin}}),
        json!({"kind":"fleet","position":{"position":7,"origin":origin}}),
    ] {
        let position = serde_json::from_value::<RecordPlacement>(encoded.clone());
        assert!(
            position.is_ok(),
            "sequenced origin was refused: {position:?}"
        );
        assert_eq!(serde_json::to_value(position.unwrap()).unwrap(), encoded);
    }
}

#[test]
fn sequenced_positions_refuse_missing_and_invalid_origins() {
    for encoded in [
        json!({"kind":"project","position":{"epoch":1,"position":7}}),
        json!({"kind":"fleet","position":7}),
        json!({"kind":"project","position":{"cursor":{"epoch":1,"position":7}}}),
        json!({"kind":"fleet","position":{"position":7}}),
        json!({"kind":"project","position":{"cursor":{"epoch":1,"position":7},
            "origin":{"installation":"not-an-installation","machine":"source machine"}}}),
        json!({"kind":"fleet","position":{"position":7,
            "origin":{"installation":"00000000-0000-4000-8000-000000000001","machine":""}}}),
    ] {
        assert!(serde_json::from_value::<RecordPlacement>(encoded).is_err());
    }
}

#[test]
fn reader_state_remains_unsequenced_without_an_origin_placeholder() {
    let encoded = json!({"kind":"unsequenced"});
    let position: RecordPlacement = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(serde_json::to_value(position).unwrap(), encoded);
    assert!(serde_json::from_value::<RecordPlacement>(json!({"kind":"unsequenced",
        "origin":{"installation":"00000000-0000-4000-8000-000000000001","machine":"source machine"}})).is_err());
}
