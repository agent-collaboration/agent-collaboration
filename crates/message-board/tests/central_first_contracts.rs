use message_board::*;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

const REQUEST: &str = "019a1234-5678-7000-8000-000000000001";
const PROJECT: &str = "019a1234-5678-7000-8000-000000000002";

#[expect(
    clippy::unwrap_used,
    reason = "Invalid test fixtures must fail immediately instead of masking the failed setup."
)]
fn rejects_obsolete_headers<T: DeserializeOwned>(base: Value) {
    assert!(serde_json::from_value::<T>(base.clone()).is_ok());
    for (field, value) in [("observedEpoch", json!(1)), ("resubmits", json!(REQUEST))] {
        let mut obsolete = base.clone();
        obsolete
            .as_object_mut()
            .unwrap()
            .insert(field.into(), value);
        assert!(
            serde_json::from_value::<T>(obsolete).is_err(),
            "{} accepted removed {field}",
            std::any::type_name::<T>()
        );
    }
}

#[test]
fn request_acknowledgement_and_initialization_headers_reject_removed_history_fields() {
    rejects_obsolete_headers::<ProjectRequest>(json!({
        "requestId":REQUEST,"projectId":PROJECT,"actor":{"kind":"human","humanId":"owner"},
        "actingFor":null,"protocolVersion":1,"operation":{"kind":"projectWatch"}
    }));
    rejects_obsolete_headers::<AcknowledgementEntry>(json!({
        "requestId":REQUEST,"scope":{"kind":"project","projectId":PROJECT},
        "through":{"epoch":1,"position":0}
    }));
    rejects_obsolete_headers::<ReadInitializationEntry>(json!({
        "requestId":REQUEST,"projectId":PROJECT
    }));
}

#[test]
fn request_decisions_have_no_voided_or_unresolved_outcome() {
    let voided = json!({"kind":"voided","cause":{"kind":"takeover","epoch":2}});
    assert!(serde_json::from_value::<RequestDecision>(voided.clone()).is_err());
    assert!(serde_json::from_value::<public_replies::RequestDecision>(voided).is_err());
    assert!(
        serde_json::from_value::<public_replies::RequestResult>(json!({
            "kind":"unresolved","decision":{"takeover":2}
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<public_replies::RequestResult>(json!({
            "kind":"notWriter","decision":{"current":2}
        }))
        .is_ok()
    );
}

#[test]
fn a_current_writer_is_never_provisional() {
    assert!(serde_json::from_value::<CurrentWriter>(json!({"kind":"provisional"})).is_err());
    assert!(serde_json::from_value::<CurrentWriter>(json!({"kind":"held","epoch":2})).is_ok());
}

#[test]
fn holder_changes_have_no_board_fact_or_router_component() {
    assert!(
        serde_json::from_value::<LifecycleFact>(json!({
            "kind":"takeover","old":1,"new":2,
            "lastReplicated":{"epoch":1,"position":0}
        }))
        .is_err()
    );
    assert!(serde_json::from_value::<RouterComponent>(json!("projectStoreRegistry")).is_err());
}
