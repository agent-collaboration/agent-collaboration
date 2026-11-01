use collaboration_protocol::{AutomationAnswer, AutomationCall};
use serde_json::json;

#[test]
fn firing_inspection_calls_and_answers_preserve_content_and_report_absence() {
    let firing = "019f0000-0000-7000-8000-000000000001";
    let automation = "019f0000-0000-7000-8000-000000000002";
    for request in [
        json!({"kind":"firingShow","value":{"firingId":firing}}),
        json!({"kind":"firingList","value":{"automationId":automation,"cursor":null,"limit":25}}),
    ] {
        let decoded: AutomationCall = serde_json::from_value(request.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), request);
    }
    let snapshot = json!({
        "firingId":firing,"automationId":automation,"dueAt":300_000,
        "revision":"019f0000-0000-7000-8000-000000000003",
        "content":{"contentId":"019f0000-0000-7000-8000-000000000004",
            "revisionId":"019f0000-0000-7000-8000-000000000005",
            "digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
        "outcome":{"state":"held"},"reportSeq":null,"batchId":null
    });
    let answer = json!({"kind":"firing","value":snapshot});
    let decoded: AutomationAnswer = serde_json::from_value(answer.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), answer);
    let page = json!({"kind":"firings","value":{"records":[snapshot],"nextCursor":null}});
    let decoded: AutomationAnswer = serde_json::from_value(page.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), page);
}

#[test]
fn firing_inspection_refuses_zero_unsafe_or_missing_report_sequence_and_off_grid_due() {
    let mut answer = json!({"kind":"firing","value":{
        "firingId":"019f0000-0000-7000-8000-000000000001",
        "automationId":"019f0000-0000-7000-8000-000000000002","dueAt":300_000,
        "revision":"019f0000-0000-7000-8000-000000000003",
        "content":{"contentId":"019f0000-0000-7000-8000-000000000004",
            "revisionId":"019f0000-0000-7000-8000-000000000005",
            "digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
        "outcome":{"state":"held"},"reportSeq":1,"batchId":null
    }});
    let schema = serde_json::to_value(schemars::schema_for!(
        collaboration_protocol::FiringSnapshot
    ))
    .unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    assert!(serde_json::from_value::<AutomationAnswer>(answer.clone()).is_ok());
    assert!(validator.is_valid(&answer["value"]));
    for invalid in [0_u64, 9_007_199_254_740_992] {
        answer["value"]["reportSeq"] = json!(invalid);
        assert!(serde_json::from_value::<AutomationAnswer>(answer.clone()).is_err());
        assert!(!validator.is_valid(&answer["value"]));
    }
    answer["value"]["reportSeq"] = json!(null);
    assert!(validator.is_valid(&answer["value"]));
    answer["value"]["dueAt"] = json!(120_000);
    assert!(serde_json::from_value::<AutomationAnswer>(answer.clone()).is_err());
    answer["value"]["dueAt"] = json!(300_000);
    answer["value"].as_object_mut().unwrap().remove("reportSeq");
    assert!(!validator.is_valid(&answer["value"]));
    assert!(serde_json::from_value::<AutomationAnswer>(answer).is_err());
}
