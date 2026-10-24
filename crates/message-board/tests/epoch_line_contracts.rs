use message_board::EpochLine;
use serde_json::json;

#[test]
fn an_epoch_line_records_its_boundary_without_a_second_holder_truth() {
    let encoded = json!({"lineId":"019f0000-0000-7000-8000-000000000001",
        "epoch":2,"inheritedThrough":41});
    let line = serde_json::from_value::<EpochLine>(encoded.clone()).unwrap();
    assert_eq!(serde_json::to_value(line).unwrap(), encoded);
    for field in ["holder", "end", "startPosition"] {
        let mut previous_shape = encoded.clone();
        previous_shape[field] = json!(null);
        assert!(serde_json::from_value::<EpochLine>(previous_shape).is_err());
    }
}

#[test]
fn epoch_line_keys_epochs_and_boundaries_are_validated() {
    let encoded = json!({"lineId":"019f0000-0000-7000-8000-000000000001",
        "epoch":2,"inheritedThrough":41});
    for (field, invalid) in [
        ("lineId", json!("00000000-0000-4000-8000-000000000001")),
        ("epoch", json!(0)),
        ("inheritedThrough", json!(-1)),
        ("inheritedThrough", json!(9223372036854775808_u64)),
    ] {
        let mut malformed = encoded.clone();
        malformed[field] = invalid;
        assert!(serde_json::from_value::<EpochLine>(malformed).is_err());
    }
}
