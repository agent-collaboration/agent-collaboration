#![allow(clippy::expect_used)]

use collaboration_central_core::AdminResult;
use serde_json::json;

#[test]
fn admin_unavailability_uses_the_same_closed_subject_contract() {
    for subject in [
        json!({"kind":"project","project":"019f0000-0000-7000-8000-000000000001"}),
        json!({"kind":"transfer","transfer":"019f0000-0000-7000-8000-000000000002"}),
    ] {
        let wire = json!({"kind":"unavailable","value":{
            "subject":subject,"reason":{"kind":"storageFull"}
        }});
        let answer: AdminResult = serde_json::from_value(wire.clone()).expect("transient answer");
        assert_eq!(serde_json::to_value(answer).expect("encode"), wire);
    }
    for subject in [
        json!({"kind":"unknown","project":"019f0000-0000-7000-8000-000000000001"}),
        json!({"kind":"project","project":"019f0000-0000-7000-8000-000000000001","extra":true}),
    ] {
        assert!(
            serde_json::from_value::<AdminResult>(json!({
                "kind":"unavailable","value":{"subject":subject,"reason":{"kind":"storageFull"}}
            }))
            .is_err()
        );
    }
}
