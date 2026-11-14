//! Complete B2 metadata through the real private Worker binding and owning SQL.
//! Signed values are compared opaquely so a failing assertion cannot print them.
use super::support::*;
use central_storage::{ReadOutcome, SqlValue};
use collaboration_central_conformance::signin_fixtures::{self as signed, NOW_SECONDS};
use serde_json::{Value, json};
use std::time::Duration;

fn number(value: i64) -> SqlValue {
    SqlValue::Integer(value.try_into().unwrap())
}

fn text(value: &str) -> SqlValue {
    SqlValue::Text(value.to_owned())
}

fn expected_metadata(compact: &str, issued_offset: i64) -> Value {
    json!({
        "digest":signed::digest(compact),
        "expiresAt":NOW_SECONDS - 60 + issued_offset + signed::LIFETIME_SECONDS,
        "signingKeyId":"stamp-a"
    })
}

fn provisioning_record_matches(row: &[SqlValue], request_id: &str, kind: &str, at_ms: i64) -> bool {
    let uuid_v7 = match row.first() {
        Some(SqlValue::Text(id)) => {
            uuid::Uuid::parse_str(id).is_ok_and(|id| id.get_version_num() == 7)
        }
        _ => false,
    };
    uuid_v7
        && row.len() == 9
        && row.get(1) == Some(&text(request_id))
        && row.get(2) == Some(&text(kind))
        && row.get(3) == Some(&text("signInProvisioning"))
        && row.get(4) == Some(&SqlValue::Null)
        && row.get(5) == Some(&SqlValue::Null)
        && row.get(6) == Some(&text(ORG))
        && row.get(7) == Some(&number(at_ms))
        && row.get(8) == Some(&number(at_ms))
}

async fn assert_stale_and_replayed_statements_preserve_rows(
    f: &Fixture,
    first: &str,
    renewal: &str,
    retained_identity: &ReadOutcome,
    retained_records: &ReadOutcome,
    retained_metadata: &Value,
) {
    let stale = signed::renewal(ORG, first, NOW_SECONDS, 11).unwrap();
    let refused = f
        .post(
            PROVISION,
            &json!({"organizationId":ORG,"organizationStatement":stale}),
        )
        .await;
    assert_eq!(refused.0, 409);
    assert!(refused.1.is_null());
    for replay in [renewal, first] {
        assert_eq!(
            f.post(
                PROVISION,
                &json!({"organizationId":ORG,"organizationStatement":replay})
            )
            .await
            .0,
            200
        );
        assert!(&f.read("SELECT id,organization_statement,created_at,updated_at FROM organization_identity ORDER BY id").await == retained_identity, "replay changed stamp identity or timestamps");
        assert!(&f.read("SELECT id,request_id,kind,origin,actor_person_id,permission_checked,target_id,created_at,updated_at FROM organization_change_records ORDER BY id").await == retained_records, "replay changed record identities or timestamps");
        let digest = f.post(DIGEST, &json!({"organizationId":ORG})).await;
        assert_eq!(digest.0, 200);
        assert!(
            &digest.1 == retained_metadata,
            "replay changed retained digest metadata"
        );
    }
}

#[tokio::test]
async fn complete_stamp_metadata_and_record_identity_survive_renewal_and_setup_replays() {
    let f = Fixture::new().await;
    let initial_at = NOW_SECONDS * 1000;
    let first = signed::stamp(ORG, NOW_SECONDS).unwrap();
    let body = json!({"organizationId":ORG,"organizationStatement":first});
    let first_reply = f.post(PROVISION, &body).await;
    assert_eq!(first_reply.0, 200);
    assert!(first_reply.1 == json!({"result":"provisioned"}));
    let initial_identity = f.read("SELECT id,organization_statement,created_at,updated_at FROM organization_identity ORDER BY id").await;
    let initial_records = f.read("SELECT id,request_id,kind,origin,actor_person_id,permission_checked,target_id,created_at,updated_at FROM organization_change_records ORDER BY id").await;
    assert_eq!(
        (initial_identity.rows.len(), initial_records.rows.len()),
        (1, 1)
    );
    assert!(
        initial_identity.rows
            == vec![vec![
                text(ORG),
                text(&first),
                number(initial_at),
                number(initial_at)
            ]],
        "the initial identity retains the exact compact and both applying timestamps"
    );
    let first_record = initial_records.rows.first().unwrap();
    assert!(
        provisioning_record_matches(first_record, ORG, "organizationProvisioned", initial_at),
        "setup record identity, request, origin, target or timestamps differ"
    );
    let digest = f.post(DIGEST, &json!({"organizationId":ORG})).await;
    assert_eq!(digest.0, 200);
    assert!(
        digest.1 == expected_metadata(&first, 0),
        "initial digest metadata differs"
    );
    assert_eq!(f.post(PROVISION, &body).await.0, 200);
    assert!(f.read("SELECT id,organization_statement,created_at,updated_at FROM organization_identity ORDER BY id").await == initial_identity, "setup repeat rewrote identity rows");
    assert!(f.read("SELECT id,request_id,kind,origin,actor_person_id,permission_checked,target_id,created_at,updated_at FROM organization_change_records ORDER BY id").await == initial_records, "setup repeat rewrote record rows");

    f.clock.advance(Duration::from_secs(10)).unwrap();
    f.runtime().await; // Transport the same scenario clock before private HTTP admission.
    let renewed_at = initial_at + 10_000;
    let renewal = signed::renewal(ORG, &first, NOW_SECONDS, 10).unwrap();
    assert_eq!(
        f.post(
            PROVISION,
            &json!({"organizationId":ORG,"organizationStatement":renewal})
        )
        .await
        .0,
        200
    );
    let renewed_identity = f.read("SELECT id,organization_statement,created_at,updated_at FROM organization_identity ORDER BY id").await;
    let renewed_records = f.read("SELECT id,request_id,kind,origin,actor_person_id,permission_checked,target_id,created_at,updated_at FROM organization_change_records ORDER BY id").await;
    assert_eq!(
        (renewed_identity.rows.len(), renewed_records.rows.len()),
        (1, 2)
    );
    assert!(
        renewed_identity.rows
            == vec![vec![
                text(ORG),
                text(&renewal),
                number(initial_at),
                number(renewed_at)
            ]],
        "replacement must retain the exact renewal and original creation time"
    );
    assert!(
        renewed_records.rows.first() == Some(first_record),
        "renewal rewrote the setup record"
    );
    let renewal_record = renewed_records.rows.get(1).unwrap();
    assert!(
        provisioning_record_matches(
            renewal_record,
            &signed::digest(&renewal),
            "stampRenewed",
            renewed_at
        ),
        "renewal record identity, request, origin, target or timestamps differ"
    );
    assert!(
        renewal_record.first() != first_record.first(),
        "renewal reused the setup record identity"
    );
    let digest = f.post(DIGEST, &json!({"organizationId":ORG})).await;
    assert_eq!(digest.0, 200);
    let retained_metadata = digest.1;
    assert!(
        retained_metadata == expected_metadata(&renewal, 10),
        "renewal digest metadata differs"
    );

    assert_stale_and_replayed_statements_preserve_rows(
        &f,
        &first,
        &renewal,
        &renewed_identity,
        &renewed_records,
        &retained_metadata,
    )
    .await;
    f.close().await;
}
