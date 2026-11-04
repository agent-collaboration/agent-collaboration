use super::*;
use serde_json::json;

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The observer's closed response must expose only the declared custody projection and reject content-bearing or invalid shapes."
)]
fn custody_response_is_closed_and_absent_rows_cannot_carry_status_or_payload()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let present = FleetMailCustodyObservation {
        outbox_count: 1,
        row: FleetMailCustodyRow::Present {
            status_kind: FleetMailStatusKind::Sent,
            status_version: StatusVersion::new(1),
            payload_present: true,
        },
    };
    let expected = json!({"outboxCount":1,"row":{"kind":"present","value":{
        "statusKind":"sent","statusVersion":1,"payloadPresent":true}}});
    assert_eq!(serde_json::to_value(&present)?, expected);
    assert_eq!(
        serde_json::from_value::<FleetMailCustodyObservation>(expected.clone())?,
        present
    );
    assert_eq!(
        serde_json::to_value(FleetMailCustodyObservation {
            outbox_count: 0,
            row: FleetMailCustodyRow::Absent
        })?,
        json!({"outboxCount":0,"row":{"kind":"absent"}})
    );
    let mut content = expected;
    content["payload"] = json!("must never be returned");
    assert!(serde_json::from_value::<FleetMailCustodyObservation>(content).is_err());
    assert!(
        serde_json::from_value::<FleetMailCustodyObservation>(json!({"outboxCount":0,
        "row":{"kind":"absent","value":{"statusKind":"sent"}}}))
        .is_err()
    );
    assert!(
        serde_json::from_value::<FleetMailCustodyRequest>(
            json!({"item":"550e8400-e29b-41d4-a716-446655440000"})
        )
        .is_err()
    );
    Ok(())
}
