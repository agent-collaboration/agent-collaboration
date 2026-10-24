use message_board::BoardOperation;
use serde_json::{Value, json};

fn recorded_actor() -> Value {
    json!({"kind":"human","humanId":"successor"})
}

#[test]
fn owner_grant_record_is_explicitly_relayed_and_closed() {
    let operation = json!({"kind":"ownerGrantRecord",
        "grantId":"01890f2e-7b4c-7cc0-98c4-000000000010",
        "grant":{"kind":"replaceLead",
            "project":"01890f2e-7b4c-7cc0-98c4-000000000001",
            "newHolder":recorded_actor(),"reason":"new commission"}});
    let decoded = serde_json::from_value::<BoardOperation>(operation.clone());
    assert!(decoded.is_ok(), "relayed grant operation: {decoded:?}");
    let mut extra = operation;
    extra["authenticated"] = json!(true);
    assert!(serde_json::from_value::<BoardOperation>(extra).is_err());
}

#[test]
fn replacement_requires_a_typed_grant_and_assignment_precondition() {
    let operation = json!({"kind":"roleReplaceLead",
        "newHolder":recorded_actor(),
        "ownerGrant":"01890f2e-7b4c-7cc0-98c4-000000000010",
        "reason":"new commission","expectedAssignmentRevision":1});
    let decoded = serde_json::from_value::<BoardOperation>(operation.clone());
    assert!(decoded.is_ok(), "lead replacement operation: {decoded:?}");
    for field in ["ownerGrant", "expectedAssignmentRevision"] {
        let mut missing = operation.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<BoardOperation>(missing).is_err());
    }
}

#[test]
fn reread_acknowledgement_carries_an_epoch_qualified_position() {
    let operation = json!({"kind":"roleAcknowledgeCurrent",
        "readThrough":{"epoch":1,"position":42}});
    let decoded = serde_json::from_value::<BoardOperation>(operation.clone());
    assert!(
        decoded.is_ok(),
        "reread acknowledgement operation: {decoded:?}"
    );
    let mut scalar = operation;
    scalar["readThrough"] = json!(42);
    assert!(serde_json::from_value::<BoardOperation>(scalar).is_err());
}
