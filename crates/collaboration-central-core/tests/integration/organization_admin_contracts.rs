//! Literal T06 admin wire oracles and serializer-generated artifact freshness.
use crate::contract_artifacts;
use collaboration_central_core::{
    OrganizationAdminResult,
    identity_contract::{AuthorizationRevision, AuthorizationRevisionAcknowledgement},
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
        let field = boundary.pointer_mut(pointer);
        assert!(
            field.is_some(),
            "description boundary fixture is missing {pointer}"
        );
        if let Some(field) = field {
            *field = json!(description);
        }
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
                {"id":"019a3dce-2000-7000-8000-000000000005","requestId":"person:fixture-operation","kind":"workspaceCreated","origin":{"kind":"person","personId":"019a3dce-2000-7000-8000-000000000006","permissionChecked":"workspace:create"},"targetId":"019a3dce-2000-7000-8000-000000000002","createdAtMillis":1900000002000_i64},
                {"id":"019a3dce-2000-7000-8000-000000000010","requestId":"019a3dce-2000-7000-8000-000000000014:machineEnrolled:019a3dce-2000-7000-8000-000000000007","kind":"machineEnrolled","origin":{"kind":"person","personId":"019a3dce-2000-7000-8000-000000000006","permissionChecked":"machine:enroll"},"targetId":"019a3dce-2000-7000-8000-000000000007","createdAtMillis":1900000003000_i64},
                {"id":"019a3dce-2000-7000-8000-000000000011","requestId":"019a3dce-2000-7000-8000-000000000015:machineRevoked:019a3dce-2000-7000-8000-000000000007","kind":"machineRevoked","origin":{"kind":"person","personId":"019a3dce-2000-7000-8000-000000000006","permissionChecked":"machine:revoke"},"targetId":"019a3dce-2000-7000-8000-000000000007","createdAtMillis":1900000004000_i64},
                {"id":"019a3dce-2000-7000-8000-000000000012","requestId":"019a3dce-2000-7000-8000-000000000007:machineExpired:019a3dce-2000-7000-8000-000000000007","kind":"machineExpired","origin":{"kind":"automatic"},"targetId":"019a3dce-2000-7000-8000-000000000007","createdAtMillis":1900000005000_i64},
                {"id":"019a3dce-2000-7000-8000-000000000013","requestId":"019a3dce-2000-7000-8000-000000000007:allowanceChanged:019a3dce-2000-7000-8000-000000000002","kind":"allowanceChanged","origin":{"kind":"automatic"},"targetId":"019a3dce-2000-7000-8000-000000000002","createdAtMillis":1900000006000_i64}
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
            json!({"result":"engineHealth","kinds":[{"kind":"wake_timer","failing":1,"retired":0,"oldestFirstFailedAt":"2030-01-01T10:05:00Z","lastFailure":"corrupt_row"},{"kind":"automation_timer","failing":0,"retired":0,"oldestFirstFailedAt":null,"lastFailure":null}],"alarm":"2030-01-01T10:10:00Z","alarmMissing":false}),
            json!({"result":"engineHealth","kinds":[],"alarm":null,"alarmMissing":true}),
            json!({"result":"engineRetired","items":[{"attemptId":"019a3dce-2000-7000-8000-000000000020","kind":"wake_timer","subjectKey":"019a3dce-2000-7000-8000-000000000021","failureClass":"corrupt_row","count":2,"retiredAt":"2030-01-01T10:10:00Z","lastFailure":"wakes.target"}],"next":"019a3dce-2000-7000-8000-000000000020"}),
            json!({"result":"engineRetired","items":[],"next":null}),
            json!({"result":"engineRetried"}),
            json!({"result":"engineDiscarded"}),
            json!({"result":"refused","reason":"notFound"}),
            json!({"result":"refused","reason":"notRetired"}),
            json!({"result":"refused","reason":"discardUnsupported"}),
            json!({"result":"refused","reason":"stale"}),
            json!({"result":"refused","reason":"subjectUnreadable"}),
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
        (
            AdminRoute::EngineHealth,
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001"}),
            json!({"v":1,"operation":"EngineHealth","organization":"019a3dce-2000-7000-8000-000000000001","target":"019a3dce-2000-7000-8000-000000000001","operationId":null,"params":{}}),
            r#"{"operation":"EngineHealth","operationId":null,"organization":"019a3dce-2000-7000-8000-000000000001","params":{},"target":"019a3dce-2000-7000-8000-000000000001","v":1}"#,
        ),
        (
            AdminRoute::EngineListRetired,
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","after":"019a3dce-2000-7000-8000-000000000020","limit":2}),
            json!({"v":1,"operation":"EngineListRetired","organization":"019a3dce-2000-7000-8000-000000000001","target":"019a3dce-2000-7000-8000-000000000001","operationId":null,"params":{"after":"019a3dce-2000-7000-8000-000000000020","limit":2}}),
            r#"{"operation":"EngineListRetired","operationId":null,"organization":"019a3dce-2000-7000-8000-000000000001","params":{"after":"019a3dce-2000-7000-8000-000000000020","limit":2},"target":"019a3dce-2000-7000-8000-000000000001","v":1}"#,
        ),
        (
            AdminRoute::EngineRetry,
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","operationId":"019a3dce-2000-7000-8000-000000000022","attemptId":"019a3dce-2000-7000-8000-000000000020"}),
            json!({"v":1,"operation":"EngineRetry","organization":"019a3dce-2000-7000-8000-000000000001","target":"019a3dce-2000-7000-8000-000000000001","operationId":"019a3dce-2000-7000-8000-000000000022","params":{"attemptId":"019a3dce-2000-7000-8000-000000000020"}}),
            r#"{"operation":"EngineRetry","operationId":"019a3dce-2000-7000-8000-000000000022","organization":"019a3dce-2000-7000-8000-000000000001","params":{"attemptId":"019a3dce-2000-7000-8000-000000000020"},"target":"019a3dce-2000-7000-8000-000000000001","v":1}"#,
        ),
        (
            AdminRoute::EngineDiscard,
            json!({"organizationId":"019a3dce-2000-7000-8000-000000000001","operationId":"019a3dce-2000-7000-8000-000000000023","attemptId":"019a3dce-2000-7000-8000-000000000020"}),
            json!({"v":1,"operation":"EngineDiscard","organization":"019a3dce-2000-7000-8000-000000000001","target":"019a3dce-2000-7000-8000-000000000001","operationId":"019a3dce-2000-7000-8000-000000000023","params":{"attemptId":"019a3dce-2000-7000-8000-000000000020"}}),
            r#"{"operation":"EngineDiscard","operationId":"019a3dce-2000-7000-8000-000000000023","organization":"019a3dce-2000-7000-8000-000000000001","params":{"attemptId":"019a3dce-2000-7000-8000-000000000020"},"target":"019a3dce-2000-7000-8000-000000000001","v":1}"#,
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

#[test]
fn authorization_revision_wire_contract() {
    // Policy digest copied from switchboard b1c9f6f616f5f77fbc87eaec877120ac06f4b1d3.
    contract_artifacts::assert_contract::<AuthorizationRevision>(
        "AuthorizationRevision",
        vec![
            json!({"kind":"organization","organizationId":"019a3dce-2000-7000-8000-000000000001","permissionPolicyDigest":"af40ec7fb999c4ef7f9611f322bab013e05666b56cd1d3101fac1e9e001d074a","revision":3,"state":"active"}),
            json!({"kind":"person","organizationId":"019a3dce-2000-7000-8000-000000000001","personId":"019a3dce-2000-7000-8000-000000000002","subjectId":"019a3dce-2000-7000-8000-000000000003","revision":4,"state":"inactive"}),
        ],
        false,
    );
}

#[test]
fn authorization_revision_acknowledgement_wire_contract() {
    contract_artifacts::assert_contract::<AuthorizationRevisionAcknowledgement>(
        "AuthorizationRevisionAcknowledgement",
        vec![json!({"acknowledgedRevision":4})],
        false,
    );
}
