use super::*;
use serde_json::json;

fn wire(operation: serde_json::Value, request_id: serde_json::Value) -> serde_json::Value {
    json!({"correlationId":"01890f2e-7b4c-7cc0-98c4-000000000001", "caller":{"kind":"session","value":{"machineId":"01890f2e-7b4c-7cc0-98c4-000000000002","sessionId":"session-1"}},"payload":{"kind":"workspace","request":{"workspaceId":"01890f2e-7b4c-7cc0-98c4-000000000003","requestId":request_id,"operation":operation}}})
}

#[test]
fn correlation_and_write_identity_are_distinct_and_reads_have_no_write_id() {
    let write = wire(
        json!({"kind":"createArea","name":"Work","description":"Where the work happens"}),
        json!("01890f2e-7b4c-7cc0-98c4-000000000004"),
    );
    let read = wire(
        json!({"kind":"list","query":{"kind":"areas"},"limit":10,"afterPosition":null}),
        serde_json::Value::Null,
    );
    let parsed: LinkRequest = serde_json::from_value(write.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), write);
    // Use the model's actual read shape, without duplicating an operation definition.
    let operation = workspace_model::WorkspaceOperation::List {
        query: workspace_model::ListQuery::Areas {
            include_archived: false,
        },
        cursor: workspace_model::ListCursor::start(workspace_model::ListOrder::OldestFirst),
        limit: workspace_model::PageLimit::try_from(10).unwrap(),
    };
    let mut read = read;
    read["payload"]["request"]["operation"] = serde_json::to_value(operation).unwrap();
    let parsed: LinkRequest = serde_json::from_value(read.clone()).unwrap();
    assert!(parsed.payload().write_request_id().is_none());
    assert_eq!(serde_json::to_value(parsed).unwrap(), read);
    read["payload"]["request"]["requestId"] = json!("01890f2e-7b4c-7cc0-98c4-000000000004");
    assert!(serde_json::from_value::<LinkRequest>(read).is_err());
    assert!(
        serde_json::from_value::<LinkRequest>(wire(
            json!({"kind":"createArea","name":"Work","description":"Where the work happens"}),
            serde_json::Value::Null
        ))
        .is_err()
    );
}

#[test]
fn untrusted_request_rejects_non_v7_unknown_fields_and_semantic_corruption() {
    let valid = wire(
        json!({"kind":"createArea","name":"Work","description":"Where the work happens"}),
        json!("01890f2e-7b4c-7cc0-98c4-000000000004"),
    );
    for (field, value) in [
        (
            "correlationId",
            json!("01890f2e-7b4c-4cc0-98c4-000000000001"),
        ),
        ("extra", json!(true)),
    ] {
        let mut invalid = valid.clone();
        invalid[field] = value;
        assert!(serde_json::from_value::<LinkRequest>(invalid).is_err());
    }
    let mut invalid = valid;
    invalid["payload"]["request"]["operation"]["name"] = json!("");
    assert!(serde_json::from_value::<LinkRequest>(invalid).is_err());
}
