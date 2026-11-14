use super::support::*;
use central_storage::SqlValue;
use collaboration_central_conformance::signin_fixtures::{self as signed, NOW_SECONDS};
use serde_json::json;
#[tokio::test]
async fn held_first_provisions_retain_the_known_second_winner_and_identical_calls_add_one_record() {
    for identical in [false, true] {
        let f = Fixture::new().await;
        let runtime = f.runtime().await;
        let first = signed::stamp(ORG, NOW_SECONDS).unwrap();
        let second = if identical {
            first.clone()
        } else {
            signed::first_statement(ORG, NOW_SECONDS, 1).unwrap()
        };
        let _: bool = runtime
            .control("hold-central", &"beforeSignInSample")
            .await
            .unwrap();
        let first_body = json!({"organizationId":ORG,"organizationStatement":first});
        let waiting = async { f.post(PROVISION, &first_body).await };
        let other = async {
            let _: bool = runtime
                .control("until-held-central", &"beforeSignInSample")
                .await
                .unwrap();
            assert_eq!(
                f.post(
                    PROVISION,
                    &json!({"organizationId":ORG,"organizationStatement":second})
                )
                .await
                .0,
                200
            );
            let committed=f.read("SELECT (SELECT json_group_array(json_array(id,organization_statement,created_at,updated_at)) FROM organization_identity),(SELECT json_group_array(json_array(id,request_id,created_at,updated_at)) FROM organization_change_records)").await;
            let _: bool = runtime
                .control("release-central", &"beforeSignInSample")
                .await
                .unwrap();
            committed
        };
        let (first_reply, committed) = tokio::join!(waiting, other);
        assert_eq!(first_reply.0, 200);
        assert!(f.read("SELECT (SELECT json_group_array(json_array(id,organization_statement,created_at,updated_at)) FROM organization_identity),(SELECT json_group_array(json_array(id,request_id,created_at,updated_at)) FROM organization_change_records)").await==committed,"released caller must retain the committed winner");
        let row = f
            .read("SELECT organization_statement FROM organization_identity")
            .await;
        assert!(matches!(&row.rows[0][0],SqlValue::Text(value)if value==&second));
        assert_eq!(
            f.read("SELECT id FROM organization_change_records")
                .await
                .rows
                .len(),
            1
        );
        f.close().await;
    }
}
#[tokio::test]
async fn held_renewal_refuses_after_second_renewal_commits_and_retains_exact_winner() {
    let f = Fixture::new().await;
    let runtime = f.runtime().await;
    let original = signed::stamp(ORG, NOW_SECONDS).unwrap();
    assert_eq!(f.provision().await.0, 200);
    let first = signed::renewal(ORG, &original, NOW_SECONDS, 1).unwrap();
    let second = signed::renewal(ORG, &original, NOW_SECONDS, 2).unwrap();
    let _: bool = runtime
        .control("hold-central", &"beforeSignInSample")
        .await
        .unwrap();
    let first_body = json!({"organizationId":ORG,"organizationStatement":first});
    let waiting = async { f.post(PROVISION, &first_body).await };
    let other = async {
        let _: bool = runtime
            .control("until-held-central", &"beforeSignInSample")
            .await
            .unwrap();
        assert_eq!(
            f.post(
                PROVISION,
                &json!({"organizationId":ORG,"organizationStatement":second})
            )
            .await
            .0,
            200
        );
        let committed=f.read("SELECT (SELECT json_group_array(json_array(id,organization_statement,created_at,updated_at)) FROM organization_identity),(SELECT json_group_array(json_array(id,request_id,created_at,updated_at)) FROM organization_change_records)").await;
        let _: bool = runtime
            .control("release-central", &"beforeSignInSample")
            .await
            .unwrap();
        committed
    };
    let (first_reply, committed) = tokio::join!(waiting, other);
    assert_eq!(first_reply.0, 409);
    assert!(f.read("SELECT (SELECT json_group_array(json_array(id,organization_statement,created_at,updated_at)) FROM organization_identity),(SELECT json_group_array(json_array(id,request_id,created_at,updated_at)) FROM organization_change_records)").await==committed,"stale held renewal must change no row");
    let rows = f
        .read("SELECT organization_statement FROM organization_identity")
        .await;
    assert!(matches!(&rows.rows[0][0],SqlValue::Text(value)if value==&second));
    assert_eq!(
        f.read("SELECT id FROM organization_change_records")
            .await
            .rows
            .len(),
        2
    );
    f.close().await;
}
