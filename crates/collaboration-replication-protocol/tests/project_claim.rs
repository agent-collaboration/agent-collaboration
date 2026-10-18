#![allow(clippy::expect_used)]
use collaboration_replication_protocol::{SessionAnswer, SessionRequest};
use serde_json::json;
#[test]
fn claim_request_and_recorded_answer_cross_the_session_wire() {
    let request = json!({"kind":"claim","value":{"attempt":"019f0000-0000-7000-8000-000000000001","project":"019f0000-0000-7000-8000-000000000002"}});
    let parsed = serde_json::from_value::<SessionRequest>(request.clone());
    assert!(parsed.is_ok(), "valid Claim rejected: {parsed:?}");
    assert_eq!(
        serde_json::to_value(parsed.expect("request")).expect("wire"),
        request
    );
    for body in [
        json!({"kind":"claimed","value":{"epoch":2,"expires_at":"2026-10-15T08:00:00Z","start":{"epoch":1,"commitId":7}}}),
        json!({"kind":"refused","value":{"kind":"alreadyHeld","holder":"019f0000-0000-7000-8000-000000000003"}}),
        json!({"kind":"refused","value":{"kind":"schemaTooNew","store":{"version":2,"fingerprint":"schema"},"supported":null}}),
        json!({"kind":"refused","value":{"kind":"notFound"}}),
        json!({"kind":"refused","value":{"kind":"notHeld"}}),
    ] {
        let parsed = serde_json::from_value::<SessionAnswer>(body.clone()).expect("claim answer");
        assert_eq!(serde_json::to_value(parsed).expect("wire"), body);
    }
    assert!(serde_json::from_value::<SessionRequest>(json!({"kind":"claim","value":{"attempt":"invalid","project":"019f0000-0000-7000-8000-000000000002"}})).is_err());
}
