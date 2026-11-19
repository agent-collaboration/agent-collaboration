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
        "SELECT * FROM signin_organization_identity",
        "SELECT * FROM signin_change_records",
        "SELECT * FROM admin_used_requests",
        "SELECT * FROM signin_authorization_floors",
        "SELECT * FROM signin_person_authorization_floors",
        "SELECT * FROM runtime_request_log",
        "SELECT * FROM directory_workspaces",
        "SELECT * FROM login_machines",
        "SELECT * FROM login_machine_approvals",
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
    let counts=f.read("SELECT (SELECT COUNT(*) FROM signin_organization_identity),(SELECT COUNT(*) FROM signin_change_records),(SELECT COUNT(*) FROM admin_used_requests),(SELECT COUNT(*) FROM signin_authorization_floors),(SELECT COUNT(*) FROM signin_person_authorization_floors),(SELECT COUNT(*) FROM runtime_request_log),(SELECT COUNT(*) FROM directory_workspaces)").await;
    assert!(
        counts.rows == vec![vec![SqlValue::Integer(0.try_into().unwrap()); 7]],
        "refused private requests wrote business, proof or decision rows"
    );
    f.close().await;
}
