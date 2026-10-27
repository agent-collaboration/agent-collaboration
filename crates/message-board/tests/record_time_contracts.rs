mod support;

use message_board::{ProjectId, RecordEnvelope, RecordId, RecordTime};
use serde_json::json;

#[test]
fn imported_unknown_time_remains_unknown_instead_of_inventing_a_timestamp() {
    // Arrange: RC-9's explicit imported-history exception, supplied as wire data.
    let encoded = json!({"kind": "legacyUnknown"});

    // Act: use the public record type's real decoder.
    let time: RecordTime = serde_json::from_value(encoded.clone()).unwrap();

    // Assert: no ID, file time or receiving clock is substituted for missing time.
    assert_eq!(time.as_router_time(), None);
    assert_eq!(serde_json::to_value(time).unwrap(), encoded);
}

#[test]
fn router_time_round_trips_the_recorded_instant() {
    let encoded = json!({"kind": "router", "value": "2026-10-18T08:00:00.123456789Z"});
    let time: RecordTime = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(
        time.as_router_time()
            .unwrap()
            .to_rfc3339_opts(chrono::SecondsFormat::Nanos, true),
        "2026-10-18T08:00:00.123456789Z"
    );
    assert_eq!(serde_json::to_value(time).unwrap(), encoded);
}

#[test]
fn malformed_record_time_is_refused() {
    for encoded in [
        json!({"kind": "router"}),
        json!({"kind": "router", "value": "unknown"}),
        json!({"kind": "legacyUnknown", "value": "2026-10-18T08:00:00Z"}),
        json!({"kind": "legacyUnknown", "invented": true}),
    ] {
        assert!(serde_json::from_value::<RecordTime>(encoded).is_err());
    }
}

#[test]
fn common_envelope_preserves_explicit_unknown_time() {
    // RC-9 and PD1's envelope put the tagged time at the common boundary,
    // including records imported without any recoverable event timestamp.
    let fixture = support::record_envelope(RecordId::generate(), ProjectId::generate());
    let mut encoded = serde_json::to_value(fixture).unwrap();
    let fields = encoded.as_object_mut().unwrap();
    fields.remove("routerTime");
    fields.insert("time".to_owned(), json!({"kind": "legacyUnknown"}));

    let envelope = serde_json::from_value::<RecordEnvelope>(encoded.clone());

    assert!(
        envelope.is_ok(),
        "tagged unknown time was refused: {envelope:?}"
    );
    assert_eq!(serde_json::to_value(envelope.unwrap()).unwrap(), encoded);
}

#[test]
fn common_envelope_refuses_the_previous_unlabelled_time_shape() {
    let fixture = support::record_envelope(RecordId::generate(), ProjectId::generate());
    let mut encoded = serde_json::to_value(fixture).unwrap();
    let fields = encoded.as_object_mut().unwrap();
    fields.remove("time");
    fields.insert("routerTime".to_owned(), json!("2026-10-12T00:00:00Z"));

    assert!(serde_json::from_value::<RecordEnvelope>(encoded).is_err());
}

#[test]
fn merge_time_places_unknown_before_every_known_time_without_inventing_an_instant() {
    use message_board::MergeTime;
    let unknown = MergeTime::from(RecordTime::LegacyUnknown);
    let before_unix_epoch =
        MergeTime::from(RecordTime::Router("1900-01-01T00:00:00Z".parse().unwrap()));
    let current = MergeTime::from(RecordTime::Router("2026-10-18T08:00:00Z".parse().unwrap()));

    assert_eq!(unknown, MergeTime::Unknown);
    assert!(unknown < before_unix_epoch);
    assert!(before_unix_epoch < current);
    assert_eq!(
        serde_json::to_value(unknown).unwrap(),
        json!({"kind":"unknown"})
    );
}
