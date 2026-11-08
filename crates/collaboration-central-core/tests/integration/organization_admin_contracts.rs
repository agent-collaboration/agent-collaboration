//! A person's three admin calls on the private link, published as JSON schema
//! and fixtures for the sign-in side to vendor. The intent vectors pin the
//! signed `intent` claim to an independent JCS implementation.
use super::contract_artifacts;
use collaboration_central_core::person_request::{
    CreateWorkspaceRequest, ReadChangeRecordsRequest, WorkspaceLookupRequest,
};
use collaboration_central_core::{OrganizationAdminResult, person_request::AdminRoute};
use serde_json::{Value, json};

const ORGANIZATION: &str = "019a3dce-2000-7000-8000-000000000001";

// Oracle: the Lead-adopted admin wire (decision a: admin routes on the private
// link; operationId is the ledger RequestId and the workspace id). Catch an
// unknown field, a missing operation identity or an out-of-range page.
#[test]
fn create_workspace_request_wire_contract() {
    contract_artifacts::assert_contract::<CreateWorkspaceRequest>(
        "CreateWorkspaceRequest",
        vec![json!({"organizationId":ORGANIZATION,
            "operationId":"019a3dce-2000-7000-8000-0000000000a1",
            "name":"research","description":""})],
        false,
    );
}

#[test]
fn workspace_lookup_request_wire_contract() {
    contract_artifacts::assert_contract::<WorkspaceLookupRequest>(
        "WorkspaceLookupRequest",
        vec![json!({"organizationId":ORGANIZATION,
            "workspaceId":"019a3dce-2000-7000-8000-0000000000a1"})],
        false,
    );
}

#[test]
fn read_change_records_request_wire_contract() {
    contract_artifacts::assert_contract::<ReadChangeRecordsRequest>(
        "ReadChangeRecordsRequest",
        vec![
            json!({"organizationId":ORGANIZATION,"after":null,"limit":100}),
            json!({"organizationId":ORGANIZATION,
                "after":"019a3dce-2000-7000-8000-0000000000b1","limit":1}),
        ],
        false,
    );
}

// The HTTP body of a 200; an unauthenticated request is a bare 401 and never
// one of these.
#[test]
fn organization_admin_result_wire_contract() {
    contract_artifacts::assert_contract::<OrganizationAdminResult>(
        "OrganizationAdminResult",
        vec![
            json!({"result":"applied","alreadyExisted":false,"workspace":{
                "workspaceId":"019a3dce-2000-7000-8000-0000000000a1","name":"research",
                "description":"","state":"active"}}),
            json!({"result":"found","workspace":{
                "workspaceId":"019a3dce-2000-7000-8000-0000000000a1","name":"research",
                "description":"","state":"active"}}),
            json!({"result":"found","workspace":null}),
            json!({"result":"records","records":[
                {"id":"019a3dce-2000-7000-8000-0000000000c1",
                    "requestId":"019a3dce-2000-7000-8000-0000000000c2",
                    "kind":"organizationProvisioned","origin":{"kind":"signInProvisioning"},
                    "targetId":ORGANIZATION,"createdAtMillis":1_800_000_000_000_i64},
                {"id":"019a3dce-2000-7000-8000-0000000000c3",
                    "requestId":"019a3dce-2000-7000-8000-0000000000a1","kind":"workspaceCreated",
                    "origin":{"kind":"person","personId":"019a3dce-2000-7000-8000-000000000002",
                        "permissionChecked":"workspace:create"},
                    "targetId":"019a3dce-2000-7000-8000-0000000000a1",
                    "createdAtMillis":1_800_000_000_001_i64}
            ]}),
            json!({"result":"refused","reason":"notPermitted"}),
            json!({"result":"refused","reason":"secondFactorRequired"}),
            json!({"result":"refused","reason":"refreshRequired"}),
            json!({"result":"refused","reason":"authorizationSyncRequired"}),
            json!({"result":"refused","reason":"expired"}),
            json!({"result":"refused","reason":"requestTime"}),
            json!({"result":"refused","reason":"requestReused"}),
            json!({"result":"refused","reason":"conflict"}),
            json!({"result":"refused","reason":"rejected"}),
        ],
        false,
    );
}

// An unauthenticated answer is not a published result.
#[test]
fn organization_admin_result_has_no_unauthenticated_body() {
    for body in [
        json!({"result":"unauthenticated"}),
        json!({"result":"refused","reason":"unauthenticated"}),
    ] {
        assert!(
            serde_json::from_value::<OrganizationAdminResult>(body.clone()).is_err(),
            "{body}"
        );
    }
}

// Oracle: the vectors file was produced by an independent JavaScript JCS
// (JSON.stringify over recursively sorted keys); central must parse each body
// into exactly that intent, canonical string and digest, or the sign-in
// service's proofs would be refused.
#[test]
fn organization_admin_intent_vectors_match_central() {
    let file: Value = serde_json::from_str(include_str!(
        "../../contracts/OrganizationAdminIntent.vectors.json"
    ))
    .unwrap();
    let vectors = file["vectors"].as_array().unwrap();
    assert_eq!(vectors.len(), 5, "keep every pinned vector");
    for vector in vectors {
        let route: AdminRoute = serde_json::from_value(vector["route"].clone()).unwrap();
        assert_eq!(route.path(), vector["path"].as_str().unwrap());
        let body = serde_json::to_vec(&vector["body"]).unwrap();
        let operation =
            collaboration_central_core::person_request::AdminOperation::parse(route, &body)
                .unwrap_or_else(|refusal| panic!("{}: {refusal:?}", vector["name"]));
        assert_eq!(operation.intent(), vector["intent"], "{}", vector["name"]);
        assert_eq!(
            operation.canonical_intent().unwrap(),
            vector["jcs"].as_str().unwrap(),
            "{}",
            vector["name"]
        );
        assert_eq!(
            operation.intent_digest().unwrap(),
            vector["intentDigest"].as_str().unwrap(),
            "{}",
            vector["name"]
        );
    }
}
