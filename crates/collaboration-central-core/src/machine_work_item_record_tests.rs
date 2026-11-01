//! Literal wire/digest oracles and corrupt stored rows exercise the real decoder.
use super::*;
use serde_json::{Value, json};
const SOURCE: &str = "019f0000-0000-7000-8000-000000000003";
const TARGET: &str = "019f0000-0000-7000-8000-000000000004";
// SHA256 independently computed over the adopted [source, full request] JSON.
const DIGEST: &str = "34320abaa7e9829e41371055bfcfcd50a88644cfb6ae24c36a36eac8885248fb";

fn entry() -> Value {
    json!({
        "workItemId":"019f0000-0000-7000-8000-000000000001",
        "operationId":"019f0000-0000-7000-8000-000000000002",
        "sourceInstallation":SOURCE,
        "request":{
            "endpoint":{"serviceId":TARGET,"endpointId":"codex-local"},
            "workingDirectory":"/fixture",
            "createdBy":{"endpoint":{"serviceId":SOURCE,"endpointId":"codex-local"},"sessionId":"requester"},
            "approver":null,"requestedPolicy":{"access":"workspace-write"},
            "settings":null,"firstInput":null
        }
    })
}
fn row() -> StoredMachineWorkItem {
    StoredMachineWorkItem {
        work_item_id: Some("019f0000-0000-7000-8000-000000000001".into()),
        operation_id: Some("019f0000-0000-7000-8000-000000000002".into()),
        service_id: Some(TARGET.into()),
        kind: Some("sessionCreate".into()),
        request_json: Some(entry().to_string()),
        request_digest: Some(DIGEST.into()),
        state: Some("pending".into()),
        result_json: None,
    }
}

#[test]
fn unreported_rows_keep_absence_separate_from_custody() {
    for state in ["pending", "held", "received"] {
        let mut stored = row();
        stored.state = Some(state.into());
        let record = decode(stored).expect("valid retained request");
        assert!(record.result.is_none());
        assert_eq!(
            serde_json::to_value(record.state).expect("state"),
            json!(state)
        );
    }
}

#[test]
fn source_and_full_request_are_in_the_literal_digest() {
    let record = decode(row()).expect("valid baseline");
    let request = RemoteSessionCreateRequest {
        operation_id: record.entry.operation_id,
        definition: record.entry.request,
    };
    assert_eq!(
        request_digest(&record.entry.source_installation, &request)
            .expect("digest")
            .as_str(),
        DIGEST
    );
    let other = "019f0000-0000-7000-8000-000000000005"
        .to_owned()
        .try_into()
        .expect("other source");
    assert_ne!(
        request_digest(&other, &request).expect("digest").as_str(),
        DIGEST
    );
    let mut changed = request;
    changed.definition.first_input = Some(
        serde_json::from_value(json!({"kind":"humanUser","text":"different full request"}))
            .expect("input"),
    );
    assert_ne!(
        request_digest(&record.entry.source_installation, &changed)
            .expect("digest")
            .as_str(),
        DIGEST
    );
    let mut stored = row();
    let mut request = entry();
    request["request"]["firstInput"] =
        json!({"kind":"humanUser","text":"changed without changing digest"});
    stored.request_json = Some(request.to_string());
    assert!(matches!(
        decode(stored),
        Err(AutomationStoreError::InvalidRecord(
            "machine_work_items.request_digest"
        ))
    ));
}

#[test]
fn row_identity_kind_state_and_digest_never_coerce() {
    #[derive(Debug)]
    enum CorruptField {
        WorkItemId,
        OperationId,
        TargetService,
        Kind,
        CustodyState,
        RequestDigest,
    }
    for field in [
        CorruptField::WorkItemId,
        CorruptField::OperationId,
        CorruptField::TargetService,
        CorruptField::Kind,
        CorruptField::CustodyState,
        CorruptField::RequestDigest,
    ] {
        let mut stored = row();
        match field {
            CorruptField::WorkItemId => {
                stored.work_item_id = Some("019f0000-0000-7000-8000-000000000009".into())
            }
            CorruptField::OperationId => {
                stored.operation_id = Some("019f0000-0000-7000-8000-000000000009".into())
            }
            CorruptField::TargetService => stored.service_id = Some(SOURCE.into()),
            CorruptField::Kind => stored.kind = Some("futureKind".into()),
            CorruptField::CustodyState => stored.state = Some("unknown".into()),
            CorruptField::RequestDigest => stored.request_digest = Some("0".repeat(64)),
        }
        assert!(
            matches!(decode(stored), Err(AutomationStoreError::InvalidRecord(_))),
            "stored {field:?} was coerced"
        );
    }
    for pointer in [
        "/workItemId",
        "/operationId",
        "/sourceInstallation",
        "/request/endpoint/serviceId",
        "/request/createdBy/endpoint/serviceId",
    ] {
        let mut stored = row();
        let mut request = entry();
        *request.pointer_mut(pointer).expect("binding") =
            json!("019f0000-0000-7000-8000-000000000009");
        stored.request_json = Some(request.to_string());
        assert!(
            matches!(decode(stored), Err(AutomationStoreError::InvalidRecord(_))),
            "stored {pointer} lost binding"
        );
    }
}

#[test]
fn stored_results_require_a_positive_sequence_and_complete_typed_fields() {
    let valid = json!({"seq":1,"create":{"state":"unknown"},"input":{"state":"unknown"}});
    let mut stored = row();
    stored.result_json = Some(valid.to_string());
    assert_eq!(
        u64::from(
            decode(stored)
                .expect("reported uncertainty")
                .result
                .expect("report")
                .seq
        ),
        1
    );
    for seq in [
        json!(0),
        json!(-1),
        json!(1.5),
        json!(9_007_199_254_740_992_u64),
    ] {
        let mut stored = row();
        let mut result = valid.clone();
        result["seq"] = seq;
        stored.result_json = Some(result.to_string());
        assert!(matches!(
            decode(stored),
            Err(AutomationStoreError::InvalidRecord(
                "machine_work_items.result_json"
            ))
        ));
    }
    for result in [
        json!(null),
        json!({}),
        json!({"seq":1,"create":{"state":"future"},"input":{"state":"none"}}),
        json!({"seq":1,"create":{"state":"unknown"},"input":{"state":"future"}}),
    ] {
        let mut stored = row();
        stored.result_json = Some(result.to_string());
        assert!(matches!(
            decode(stored),
            Err(AutomationStoreError::InvalidRecord(
                "machine_work_items.result_json"
            ))
        ));
    }
}

#[test]
fn known_created_session_belongs_to_the_bound_target() {
    let mut result = json!({"seq":1,"create":{"state":"created","sessionRef":{
        "endpoint":{"serviceId":TARGET,"endpointId":"codex-local"},"sessionId":"created-target"
    },"settings":{"state":"applied","effectiveSettings":null,"permissionDifferences":[]}},"input":{"state":"accepted"}});
    let mut stored = row();
    stored.result_json = Some(result.to_string());
    assert!(decode(stored).is_ok());
    result["create"]["sessionRef"]["endpoint"]["serviceId"] = json!(SOURCE);
    let mut stored = row();
    stored.result_json = Some(result.to_string());
    assert!(matches!(
        decode(stored),
        Err(AutomationStoreError::InvalidRecord(
            "machine_work_items.result_json"
        ))
    ));
}

#[test]
fn every_required_stored_column_refuses_null() {
    #[derive(Debug)]
    enum RequiredColumn {
        Item,
        Operation,
        Target,
        Kind,
        Request,
        Digest,
        Custody,
    }
    for column in [
        RequiredColumn::Item,
        RequiredColumn::Operation,
        RequiredColumn::Target,
        RequiredColumn::Kind,
        RequiredColumn::Request,
        RequiredColumn::Digest,
        RequiredColumn::Custody,
    ] {
        let mut stored = row();
        match column {
            RequiredColumn::Item => stored.work_item_id = None,
            RequiredColumn::Operation => stored.operation_id = None,
            RequiredColumn::Target => stored.service_id = None,
            RequiredColumn::Kind => stored.kind = None,
            RequiredColumn::Request => stored.request_json = None,
            RequiredColumn::Digest => stored.request_digest = None,
            RequiredColumn::Custody => stored.state = None,
        }
        assert!(
            matches!(decode(stored), Err(AutomationStoreError::InvalidRecord(_))),
            "null {column:?} was silently filled"
        );
    }
    // result_json is the only nullable column; absence must remain unreported.
    assert!(decode(row()).expect("nullable result").result.is_none());
}

#[test]
fn required_request_fields_refuse_missing_null_and_wrong_types() {
    for pointer in [
        "/workItemId",
        "/operationId",
        "/sourceInstallation",
        "/request",
        "/request/endpoint",
        "/request/endpoint/serviceId",
        "/request/endpoint/endpointId",
        "/request/workingDirectory",
        "/request/createdBy",
        "/request/createdBy/endpoint",
        "/request/createdBy/endpoint/serviceId",
        "/request/createdBy/endpoint/endpointId",
        "/request/createdBy/sessionId",
        "/request/requestedPolicy",
        "/request/requestedPolicy/access",
    ] {
        for invalid in [None, Some(Value::Null), Some(json!(37))] {
            let mut request = entry();
            replace_or_remove(&mut request, pointer, invalid);
            let mut stored = row();
            stored.request_json = Some(request.to_string());
            assert!(
                matches!(decode(stored), Err(AutomationStoreError::InvalidRecord(_))),
                "malformed {pointer} was coerced"
            );
        }
    }
    for malformed in ["", "{", "null", "[]", "37"] {
        let mut stored = row();
        stored.request_json = Some(malformed.into());
        assert!(matches!(
            decode(stored),
            Err(AutomationStoreError::InvalidRecord(_))
        ));
    }
}

#[test]
fn result_variants_refuse_incomplete_nested_fields_and_unrecognized_fields() {
    let created = json!({"seq":1,"create":{"state":"created","sessionRef":{
        "endpoint":{"serviceId":TARGET,"endpointId":"codex-local"},"sessionId":"target"
    },"settings":{"state":"applied","effectiveSettings":null,"permissionDifferences":[]}},"input":{"state":"accepted"}});
    let refused = json!({"seq":1,"create":{"state":"refused","reason":{"kind":"approverRequired"}},
        "input":{"state":"failed","reason":"fixture input failure"}});
    for (baseline, pointers) in [
        (
            &created,
            vec![
                "/seq",
                "/create",
                "/input",
                "/create/state",
                "/input/state",
                "/create/sessionRef",
                "/create/sessionRef/endpoint",
                "/create/sessionRef/endpoint/serviceId",
                "/create/sessionRef/endpoint/endpointId",
                "/create/sessionRef/sessionId",
                "/create/settings",
                "/create/settings/state",
                "/create/settings/permissionDifferences",
            ],
        ),
        (
            &refused,
            vec!["/create/reason", "/create/reason/kind", "/input/reason"],
        ),
    ] {
        let mut valid = row();
        valid.result_json = Some(baseline.to_string());
        assert!(decode(valid).is_ok(), "matrix baseline must decode");
        for pointer in pointers {
            for invalid in [None, Some(Value::Null), Some(json!(true))] {
                let mut result = baseline.clone();
                replace_or_remove(&mut result, pointer, invalid);
                let mut stored = row();
                stored.result_json = Some(result.to_string());
                assert!(
                    matches!(
                        decode(stored),
                        Err(AutomationStoreError::InvalidRecord(
                            "machine_work_items.result_json"
                        ))
                    ),
                    "incomplete result {pointer} was coerced"
                );
            }
        }
        for pointer in ["", "/create", "/input"] {
            let mut result = baseline.clone();
            result
                .pointer_mut(pointer)
                .and_then(Value::as_object_mut)
                .expect("variant object")
                .insert("unexpected".into(), json!(true));
            let mut stored = row();
            stored.result_json = Some(result.to_string());
            assert!(
                matches!(
                    decode(stored),
                    Err(AutomationStoreError::InvalidRecord(
                        "machine_work_items.result_json"
                    ))
                ),
                "unknown result field accepted at {pointer}; baseline={baseline}"
            );
        }
    }
}

fn replace_or_remove(value: &mut Value, pointer: &str, replacement: Option<Value>) {
    let (parent, key) = pointer.rsplit_once('/').expect("field pointer");
    let fields = value
        .pointer_mut(parent)
        .and_then(Value::as_object_mut)
        .expect("field owner");
    if let Some(replacement) = replacement {
        fields.insert(key.into(), replacement);
    } else {
        assert!(fields.remove(key).is_some(), "matrix field must exist");
    }
}
