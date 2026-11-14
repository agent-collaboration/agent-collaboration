//! The active private surface refuses unstamped reads, creates and revisions without a body.
use super::support::*;
use serde_json::json;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Exact private status/body and owning-row assertions propagate fixture failures."
)]
#[tokio::test]
async fn unstamped_private_reads_and_revisions_reveal_nothing_and_unknown_routes_remain_absent()
-> Result {
    let f = Fixture::new().await?;
    let person = signed::Person::owner(A, signed::NOW_SECONDS)?;
    for (path, body, intent) in [
        (
            signed::CREATE,
            signed::create_body(A, "019f0000-0000-7000-8000-00000000d7c1", "Unstamped"),
            signed::create_intent(A, "019f0000-0000-7000-8000-00000000d7c1", "Unstamped"),
        ),
        (
            signed::RECORDS,
            json!({"organizationId":A,"after":null,"limit":100}),
            signed::records_intent(A, None, 100),
        ),
        (
            signed::LOOKUP,
            json!({"organizationId":A,"workspaceId":WORKSPACE}),
            signed::lookup_intent(A, WORKSPACE),
        ),
    ] {
        let response = f
            .person(
                path,
                &body,
                &person,
                &intent,
                &signed::fresh_request_id(signed::NOW_SECONDS)?,
            )
            .await?;
        assert_eq!(response.status, 503);
        assert!(
            response.body.is_empty(),
            "an unstamped signed read revealed a response payload"
        );
    }
    let revision = f
        .post(REVISIONS, &organization_floor(A, 1, "active"))
        .await?;
    assert_eq!(revision.status, 503);
    assert!(
        revision.body.is_empty(),
        "an unstamped revision revealed a response payload"
    );
    let digest = f.digest(A).await?;
    assert_eq!(digest.status, 404);
    assert!(
        digest.body.is_empty(),
        "an unknown private digest revealed a response payload"
    );
    let unknown = f
        .post(
            "/internal/no-such-signin-route",
            &json!({"organizationId":A}),
        )
        .await?;
    assert_eq!(unknown.status, 404);
    assert!(
        unknown.body.is_empty(),
        "an unknown private route revealed a response payload"
    );
    let counts=f.rows(A,"SELECT (SELECT COUNT(*) FROM organization_identity),(SELECT COUNT(*) FROM organization_change_records),(SELECT COUNT(*) FROM organization_used_requests),(SELECT COUNT(*) FROM organization_authorization_floors),(SELECT COUNT(*) FROM person_organization_authorization_floors),(SELECT COUNT(*) FROM request_log),(SELECT COUNT(*) FROM workspaces)",vec![]).await?;
    assert!(
        counts == vec![vec![number(0)?; 7]],
        "refused private requests wrote business, proof or decision rows"
    );
    f.close().await
}
