//! Active private Worker routes and the Lead-settled public refusal boundary.
//! Signed Create joins reads and revisions on the approved private setup surface.
use super::support::*;
use central_storage::{ReadOutcome, SqlValue};
use collaboration_central_conformance::signin_fixtures::{self as signed, NOW_SECONDS};
use serde_json::{Value, json};

async fn private_reply(
    f: &Fixture,
    path: &str,
    body: &Value,
    headers: &[(&str, String)],
) -> (u16, Vec<u8>) {
    let mut request = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("{}{path}", f.host.signin_origin()))
        .json(body);
    for (name, value) in headers {
        request = request.header(*name, value);
    }
    let response = request.send().await.unwrap();
    assert!(
        response
            .headers()
            .get("x-collaboration-central-entry")
            .is_some_and(|value| value == "collaboration-central-link-entry"),
        "request did not pass through the product private entry"
    );
    (
        response.status().as_u16(),
        response.bytes().await.unwrap().to_vec(),
    )
}

async fn owning_rows(f: &Fixture) -> Vec<ReadOutcome> {
    let mut rows = Vec::new();
    for sql in [
        "SELECT * FROM organization_identity",
        "SELECT * FROM organization_change_records",
        "SELECT * FROM organization_used_requests",
        "SELECT * FROM organization_authorization_floors",
        "SELECT * FROM person_organization_authorization_floors",
        "SELECT * FROM request_log",
        "SELECT * FROM workspaces",
        "SELECT * FROM machines",
        "SELECT * FROM machine_credentials",
    ] {
        rows.push(f.read(sql).await);
    }
    rows
}

#[tokio::test]
async fn unstamped_private_reads_and_revisions_reveal_nothing_and_unknown_routes_remain_absent() {
    let f = Fixture::new().await;
    let person = f.owner();
    let unknown_workspace = "019f0000-0000-7000-8000-00000000d298";
    let before = owning_rows(&f).await;
    for (path, body, intent) in [
        (
            signed::CREATE,
            signed::create_body(ORG, unknown_workspace, "Unstamped create"),
            signed::create_intent(ORG, unknown_workspace, "Unstamped create"),
        ),
        (
            RECORDS,
            json!({"organizationId":ORG,"after":null,"limit":100}),
            signed::records_intent(ORG, None, 100),
        ),
        (
            LOOKUP,
            json!({"organizationId":ORG,"workspaceId":unknown_workspace}),
            signed::lookup_intent(ORG, unknown_workspace),
        ),
    ] {
        let headers = person
            .headers(
                path,
                &intent,
                &signed::fresh_request_id(NOW_SECONDS).unwrap(),
                NOW_SECONDS,
            )
            .unwrap();
        let response = private_reply(&f, path, &body, &headers).await;
        assert_eq!(response.0, 503);
        assert!(
            response.1.is_empty(),
            "an unstamped signed operation revealed a response payload"
        );
        assert!(
            owning_rows(&f).await == before,
            "an unstamped signed operation changed the owning store"
        );
    }
    let revision = private_reply(&f, REVISIONS, &org_revision(1, "active"), &[]).await;
    assert_eq!(revision.0, 503);
    assert!(
        revision.1.is_empty(),
        "an unstamped revision revealed a response payload"
    );
    let digest = private_reply(&f, DIGEST, &json!({"organizationId":ORG}), &[]).await;
    assert_eq!(digest.0, 404);
    assert!(
        digest.1.is_empty(),
        "an unknown private digest revealed a response payload"
    );
    let unknown = private_reply(
        &f,
        "/internal/no-such-signin-route",
        &json!({"organizationId":ORG}),
        &[],
    )
    .await;
    assert_eq!(unknown.0, 404);
    assert!(
        unknown.1.is_empty(),
        "an unknown private route revealed a response payload"
    );
    let counts=f.read("SELECT (SELECT COUNT(*) FROM organization_identity),(SELECT COUNT(*) FROM organization_change_records),(SELECT COUNT(*) FROM organization_used_requests),(SELECT COUNT(*) FROM organization_authorization_floors),(SELECT COUNT(*) FROM person_organization_authorization_floors),(SELECT COUNT(*) FROM request_log),(SELECT COUNT(*) FROM workspaces)").await;
    assert!(
        counts.rows == vec![vec![SqlValue::Integer(0.try_into().unwrap()); 7]],
        "refused private requests wrote business, proof or decision rows"
    );
    f.close().await;
}

#[tokio::test]
async fn every_active_private_path_is_publicly_refused_before_and_after_an_access_assertion() {
    let f = Fixture::new().await;
    let statement = signed::stamp(ORG, NOW_SECONDS).unwrap();
    let body = json!({"organizationId":ORG,"organizationStatement":statement});
    assert_eq!(f.post(PROVISION, &body).await.0, 200);
    let before = owning_rows(&f).await;
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let owner = collaboration_central_conformance::owner_assertion().unwrap();
    // Only the old public404 clause changes, by Lead01a1d910-44a9: current
    // public401 carries missingAssertion or routeRefused; private404 is kept.
    let create_operation = "019f0000-0000-7000-8000-00000000d299";
    let person = f.owner();
    for (path, request_body, proof) in [
        (PROVISION, body.clone(), vec![]),
        (DIGEST, json!({"organizationId":ORG}), vec![]),
        (REVISIONS, org_revision(3, "active"), vec![]),
        (
            LOOKUP,
            json!({"organizationId":ORG,"workspaceId":create_operation}),
            vec![],
        ),
        (
            RECORDS,
            json!({"organizationId":ORG,"after":null,"limit":100}),
            vec![],
        ),
        (
            signed::CREATE,
            signed::create_body(ORG, create_operation, "Publicly refused create"),
            person
                .headers(
                    signed::CREATE,
                    &signed::create_intent(ORG, create_operation, "Publicly refused create"),
                    &signed::fresh_request_id(NOW_SECONDS).unwrap(),
                    NOW_SECONDS,
                )
                .unwrap(),
        ),
    ] {
        for present_assertion in [false, true] {
            let mut request = client
                .post(format!("{}{path}", f.host.public_origin()))
                .json(&request_body);
            for (name, value) in &proof {
                request = request.header(*name, value);
            }
            if present_assertion {
                request = request.header("Cf-Access-Jwt-Assertion", &owner);
            }
            let response = request.send().await.unwrap();
            assert_eq!(response.status().as_u16(), 401);
            let response: Value = response.json().await.unwrap();
            assert!(
                response
                    == json!({"kind":if present_assertion{"routeRefused"}else{"missingAssertion"}}),
                "public refusal classification differs"
            );
            assert!(
                owning_rows(&f).await == before,
                "a public request changed the owning modern store"
            );
        }
    }
    f.close().await;
}
