//! Literal T06 admin wire oracles and serializer-generated artifact freshness.
use crate::contract_artifacts;
use collaboration_central_core::{
    OrganizationAdminResult,
    person_request::{
        AdminOperation, AdminRoute, CreateWorkspaceRequest, OrganizationAdminIntent,
        ReadChangeRecordsRequest, WorkspaceLookupRequest,
    },
};
use serde_json::{Value, json};

fn description_boundaries(literal: Value, pointer: &str) -> Vec<Value> {
    [
        String::new(),
        "a".repeat(2048),
        "é".repeat(1024),
        "😀".repeat(512),
    ]
    .into_iter()
    .map(|description| {
        assert!(description.is_empty() || description.len() == 2048);
        let mut boundary = literal.clone();
        *boundary.pointer_mut(pointer).unwrap() = json!(description);
        boundary
    })
    .collect()
}

#[test]
fn organization_admin_result_wire_contract() {
    let mut boundaries = description_boundaries(
        json!({"result":"applied","workspace":{"workspaceId":"019a3dce-2000-7000-8000-000000000002","name":"Fixture workspace","description":"Fixture description","state":"active"}}),
        "/workspace/description",
    );
    boundaries.extend(description_boundaries(
        json!({"result":"found","workspace":{"workspaceId":"019a3dce-2000-7000-8000-000000000002","name":"Fixture workspace","description":"Fixture description","state":"active"}}),
        "/workspace/description",
    ));
    contract_artifacts::assert_contract::<OrganizationAdminResult>(
        "OrganizationAdminResult",
        vec![
            json!({"result":"applied","workspace":{"workspaceId":"019a3dce-2000-7000-8000-000000000002","name":"Fixture workspace","description":"Fixture description","state":"active"}}),
            json!({"result":"found","workspace":{"workspaceId":"019a3dce-2000-7000-8000-000000000002","name":"Fixture workspace","description":"Fixture description","state":"archiving"}}),
            json!({"result":"found","workspace":null}),
            json!({"result":"records","records":[]}),
            json!({"result":"records","records":[
                {"id":"019a3dce-2000-7000-8000-000000000003","requestId":"stamp:digest-fixture","kind":"organizationProvisioned","origin":{"kind":"signInProvisioning"},"targetId":"019a3dce-2000-7000-8000-000000000001","createdAtMillis":1900000000000_i64},
                {"id":"019a3dce-2000-7000-8000-000000000004","requestId":"renewal:digest-fixture","kind":"stampRenewed","origin":{"kind":"signInProvisioning"},"targetId":"019a3dce-2000-7000-8000-000000000001","createdAtMillis":1900000001000_i64},
                {"id":"019a3dce-2000-7000-8000-000000000005","requestId":"person:fixture-operation","kind":"workspaceCreated","origin":{"kind":"person","personId":"019a3dce-2000-7000-8000-000000000006","permissionChecked":"workspace:create"},"targetId":"019a3dce-2000-7000-8000-000000000002","createdAtMillis":1900000002000_i64}
            ]}),
            json!({"result":"refused","reason":"nameInUse"}),
            json!({"result":"refused","reason":"conflict"}),
            json!({"result":"refused","reason":"notPermitted"}),
            json!({"result":"refused","reason":"secondFactorRequired"}),
            json!({"result":"refused","reason":"refreshRequired"}),
            json!({"result":"refused","reason":"authorizationSyncRequired"}),
            json!({"result":"refused","reason":"expired"}),
            json!({"result":"refused","reason":"requestTime"}),
            json!({"result":"refused","reason":"requestReused"}),
            json!({"result":"refused","reason":"rejected"}),
        ].into_iter().chain(boundaries).collect(),
        false,
    );
}

#[test]
fn create_workspace_request_wire_contract() {
    let boundaries = description_boundaries(
        json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","operationId":"019a3dce-2000-7000-8000-000000000003","name":"Fixture workspace","description":"Fixture description"}),
        "/description",
    );
    contract_artifacts::assert_contract::<CreateWorkspaceRequest>(
        "CreateWorkspaceRequest",
        vec![
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","operationId":"019a3dce-2000-7000-8000-000000000003","name":"Fixture workspace","description":"Fixture description"}),
        ].into_iter().chain(boundaries).collect(),
        false,
    );
}

#[test]
fn workspace_lookup_request_wire_contract() {
    contract_artifacts::assert_contract::<WorkspaceLookupRequest>(
        "WorkspaceLookupRequest",
        vec![
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","workspaceId":"019a3dce-2000-7000-8000-000000000002"}),
        ],
        false,
    );
}

#[test]
fn read_change_records_request_wire_contract() {
    // Existing exports describe serializer output. An omitted input cursor
    // remains accepted and serializes as null; the artifact is not an input validator.
    let absent: ReadChangeRecordsRequest = serde_json::from_value(json!({
        "organizationId":"019a3dce-2000-7000-8000-000000000001","limit":1
    }))
    .unwrap();
    assert!(absent.after.is_none());
    assert_eq!(
        serde_json::to_value(absent).unwrap(),
        json!({
            "organizationId":"019a3dce-2000-7000-8000-000000000001","after":null,"limit":1
        })
    );
    contract_artifacts::assert_contract::<ReadChangeRecordsRequest>(
        "ReadChangeRecordsRequest",
        vec![
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","after":null,"limit":1}),
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","after":"019a3dce-2000-7000-8000-000000000003","limit":100}),
        ],
        false,
    );
}

#[test]
fn organization_admin_intent_wire_contract() {
    // Arrange: independent wire and canonical byte oracles for each existing route.
    let cases = [
        (
            AdminRoute::CreateWorkspace,
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","operationId":"019a3dce-2000-7000-8000-000000000003","name":"Fixture workspace","description":"Fixture description"}),
            json!({"v":1,"operation":"CreateWorkspace","organization":"019a3dce-2000-7000-8000-000000000001","target":"019a3dce-2000-7000-8000-000000000001","operationId":"019a3dce-2000-7000-8000-000000000003","params":{"name":"Fixture workspace","description":"Fixture description"}}),
            r#"{"operation":"CreateWorkspace","operationId":"019a3dce-2000-7000-8000-000000000003","organization":"019a3dce-2000-7000-8000-000000000001","params":{"description":"Fixture description","name":"Fixture workspace"},"target":"019a3dce-2000-7000-8000-000000000001","v":1}"#,
        ),
        (
            AdminRoute::WorkspaceLookup,
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","workspaceId":"019a3dce-2000-7000-8000-000000000002"}),
            json!({"v":1,"operation":"WorkspaceLookup","organization":"019a3dce-2000-7000-8000-000000000001","target":"019a3dce-2000-7000-8000-000000000002","operationId":null,"params":{}}),
            r#"{"operation":"WorkspaceLookup","operationId":null,"organization":"019a3dce-2000-7000-8000-000000000001","params":{},"target":"019a3dce-2000-7000-8000-000000000002","v":1}"#,
        ),
        (
            AdminRoute::ReadChangeRecords,
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","after":null,"limit":1}),
            json!({"v":1,"operation":"ReadChangeRecords","organization":"019a3dce-2000-7000-8000-000000000001","target":"019a3dce-2000-7000-8000-000000000001","operationId":null,"params":{"after":null,"limit":1}}),
            r#"{"operation":"ReadChangeRecords","operationId":null,"organization":"019a3dce-2000-7000-8000-000000000001","params":{"after":null,"limit":1},"target":"019a3dce-2000-7000-8000-000000000001","v":1}"#,
        ),
    ];
    let mut literals = Vec::new();
    for (route, body, literal, canonical) in cases {
        // Act: the actual operation constructs the typed intent used by its proof.
        let operation = AdminOperation::parse(route, &serde_json::to_vec(&body).unwrap()).unwrap();
        let intent = operation.intent();

        // Assert: typing the existing object changes neither shape nor signed bytes.
        assert_eq!(serde_json::to_value(intent).unwrap(), literal);
        assert_eq!(operation.canonical_intent().unwrap(), canonical);
        literals.push(literal);
    }
    literals.extend(description_boundaries(
        literals[0].clone(),
        "/params/description",
    ));
    contract_artifacts::assert_contract::<OrganizationAdminIntent>(
        "OrganizationAdminIntent",
        literals,
        false,
    );
}
