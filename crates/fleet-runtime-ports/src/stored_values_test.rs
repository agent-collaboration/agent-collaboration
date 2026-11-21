use super::*;

#[test]
fn stored_timestamps_are_utc_with_exactly_three_fraction_digits() {
    let cases = [
        ("2026-11-20T06:00:00Z", "2026-11-20T06:00:00.000Z"),
        ("2026-11-20T06:00:00.123999Z", "2026-11-20T06:00:00.123Z"),
        (
            "2026-11-20T01:00:00.324999-05:00",
            "2026-11-20T06:00:00.324Z",
        ),
    ];
    for (input, expected) in cases {
        let utc = input.parse::<DateTime<Utc>>().unwrap();
        assert_eq!(format_utc_millis(utc), expected, "{input}");
    }
}

struct RejectSerialization;

impl Serialize for RejectSerialization {
    fn serialize<S: serde::Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom("fixture rejects serialization"))
    }
}

#[test]
fn stored_json_keeps_the_serialized_text_and_the_callers_failure_label() {
    let encoded = encode_json(&["queued", "notice"], "mail JSON");
    assert_eq!(encoded, Ok(SqlValue::Text(r#"["queued","notice"]"#.into())));

    for field in ["mail JSON", "another caller's field"] {
        let failure = encode_json(&RejectSerialization, field);
        assert_eq!(
            failure,
            Err(StoreFailure::Corrupt {
                reason: CorruptReason::BadValue {
                    column: field.into(),
                },
            }),
        );
    }
}
