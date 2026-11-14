use super::support::*;
use serde_json::json;
#[tokio::test]
async fn revision_routes_commit_before_ack_and_never_lower_or_revive_deleted_floors() {
    let f = Fixture::new().await;
    assert_eq!(f.provision().await.0, 200);
    assert_eq!(
        f.post(REVISIONS, &person_revision(4, "active")).await,
        (412, serde_json::Value::Null)
    );
    assert_eq!(
        f.post(REVISIONS, &org_revision(3, "active")).await,
        (200, json!({"acknowledgedRevision":3}))
    );
    assert_eq!(
        f.post(REVISIONS, &org_revision(2, "stopped")).await,
        (200, json!({"acknowledgedRevision":3}))
    );
    let before = f.counts().await;
    assert_eq!(
        f.post(REVISIONS, &org_revision(3, "stopped")).await,
        (409, serde_json::Value::Null)
    );
    assert_eq!(f.counts().await, before);
    assert_eq!(
        f.post(REVISIONS, &person_revision(4, "active")).await,
        (200, json!({"acknowledgedRevision":4}))
    );
    assert_eq!(
        f.post(REVISIONS, &person_revision(1, "inactive")).await,
        (200, json!({"acknowledgedRevision":4}))
    );
    assert_eq!(
        f.post(REVISIONS, &person_revision(4, "inactive")).await,
        (409, serde_json::Value::Null)
    );
    assert_eq!(f.post(REVISIONS, &org_revision(5, "deleted")).await.0, 200);
    assert_eq!(
        f.post(REVISIONS, &org_revision(6, "active")).await,
        (200, json!({"acknowledgedRevision":5}))
    );
    let rows = f
        .read("SELECT minimum_revision,state FROM organization_authorization_floors")
        .await;
    assert_eq!(
        rows.rows,
        vec![vec![
            central_storage::SqlValue::Integer(5.try_into().unwrap()),
            central_storage::SqlValue::Text("deleted".to_owned())
        ]]
    );
    f.close().await;
}
