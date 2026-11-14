use super::support::*;
use collaboration_central_conformance::signin_fixtures::{self as stamp, NOW_SECONDS};
use serde_json::json;
#[tokio::test]
async fn private_provision_digest_repeat_and_predecessor_renewal_are_atomic() {
    let f = Fixture::new().await;
    assert_eq!(
        f.post(DIGEST, &json!({"organizationId":ORG})).await,
        (404, serde_json::Value::Null)
    );
    let first = stamp::stamp(ORG, NOW_SECONDS).unwrap();
    let body = json!({"organizationId":ORG,"organizationStatement":first});
    assert_eq!(
        f.post(PROVISION, &body).await,
        (200, json!({"result":"provisioned"}))
    );
    assert_eq!(
        f.post(PROVISION, &body).await,
        (200, json!({"result":"provisioned"}))
    );
    let digest = f.post(DIGEST, &json!({"organizationId":ORG})).await;
    assert_eq!(digest.0, 200);
    assert_eq!(digest.1["digest"], stamp::digest(&first));
    let before = f.counts().await;
    let alternate = stamp::first_statement(ORG, NOW_SECONDS, 1).unwrap();
    assert_eq!(
        f.post(
            PROVISION,
            &json!({"organizationId":ORG,"organizationStatement":alternate})
        )
        .await
        .0,
        200
    );
    let invalid = stamp::tamper_signature(&first).unwrap();
    assert_eq!(
        f.post(
            PROVISION,
            &json!({"organizationId":ORG,"organizationStatement":invalid})
        )
        .await
        .0,
        422
    );
    assert_eq!(f.counts().await, before);
    let renewed = stamp::renewal(ORG, &first, NOW_SECONDS, 1).unwrap();
    assert_eq!(
        f.post(
            PROVISION,
            &json!({"organizationId":ORG,"organizationStatement":renewed})
        )
        .await
        .0,
        200
    );
    let stale = stamp::renewal(ORG, &first, NOW_SECONDS, 2).unwrap();
    assert_eq!(
        f.post(
            PROVISION,
            &json!({"organizationId":ORG,"organizationStatement":stale})
        )
        .await
        .0,
        409
    );
    let records = f
        .read("SELECT kind,COUNT(*) FROM organization_change_records GROUP BY kind ORDER BY kind")
        .await;
    assert_eq!(records.rows.len(), 2);
    // Lead01a1d910-44a9 preserves current 401 public refusal, superseding B2 404.
    let public = reqwest::Client::new()
        .post(format!("{}{PROVISION}", f.host.public_origin()))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(public.status(), 401);
    let public_body: serde_json::Value = public.json().await.unwrap();
    assert_eq!(public_body, serde_json::json!({"kind":"missingAssertion"}));
    f.close().await;
}
#[tokio::test]
async fn wrong_org_unpublished_and_expired_stamps_do_not_write() {
    let f = Fixture::new().await;
    let other = "019f0000-0000-7000-8000-00000000d299";
    let stamp = stamp::stamp(other, NOW_SECONDS).unwrap();
    assert_eq!(
        f.post(
            PROVISION,
            &json!({"organizationId":ORG,"organizationStatement":stamp})
        )
        .await
        .0,
        403
    );
    let unpublished =
        stamp::sign(&stamp::stamp_header(), &stamp::claims(ORG, NOW_SECONDS), 9).unwrap();
    assert_eq!(
        f.post(
            PROVISION,
            &json!({"organizationId":ORG,"organizationStatement":unpublished})
        )
        .await
        .0,
        422
    );
    let expired = stamp::stamp(ORG, NOW_SECONDS - stamp::LIFETIME_SECONDS).unwrap();
    assert_eq!(
        f.post(
            PROVISION,
            &json!({"organizationId":ORG,"organizationStatement":expired})
        )
        .await
        .0,
        422
    );
    let rows=f.read("SELECT (SELECT COUNT(*) FROM organization_identity),(SELECT COUNT(*) FROM organization_change_records)").await;
    assert_eq!(
        rows.rows,
        vec![vec![
            central_storage::SqlValue::Integer(0.try_into().unwrap()),
            central_storage::SqlValue::Integer(0.try_into().unwrap())
        ]]
    );
    f.close().await;
}
#[tokio::test]
async fn cold_tampered_stamp_refuses_and_late_record_failure_rolls_back_stamp() {
    use central_storage::{
        AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, SqlValue, Statement,
        StatementBatch,
    };
    let f = Fixture::new().await;
    let runtime = f.runtime().await;
    let trigger = StatementBatch {
        statements: vec![GuardedStatement {
            statement: Statement {
                sql: "CREATE TRIGGER fail_provision_record BEFORE INSERT ON organization_change_records BEGIN SELECT RAISE(ABORT,'fixture record failure'); END",
                args: vec![],
            },
            expect: RowExpectation::Any,
        }],
        alarm: AlarmUpdate::Unchanged,
    };
    assert!(matches!(
        runtime.run_batch(trigger).await.unwrap(),
        BatchOutcome::Applied { .. }
    ));
    assert_eq!(f.provision().await.0, 503);
    assert_eq!(
        f.post(DIGEST, &json!({"organizationId":ORG})).await,
        (404, serde_json::Value::Null),
        "the rolled-back first stamp must remain absent through its private read"
    );
    let rows=f.read("SELECT (SELECT COUNT(*) FROM organization_identity),(SELECT COUNT(*) FROM organization_change_records)").await;
    assert_eq!(
        rows.rows,
        vec![vec![
            SqlValue::Integer(0.try_into().unwrap()),
            SqlValue::Integer(0.try_into().unwrap())
        ]]
    );
    assert!(matches!(
        runtime
            .run_batch(StatementBatch {
                statements: vec![GuardedStatement {
                    statement: Statement {
                        sql: "DROP TRIGGER fail_provision_record",
                        args: vec![]
                    },
                    expect: RowExpectation::Any
                }],
                alarm: AlarmUpdate::Unchanged
            })
            .await
            .unwrap(),
        BatchOutcome::Applied { .. }
    ));
    assert_eq!(f.provision().await.0, 200);
    let original = stamp::stamp(ORG, NOW_SECONDS).unwrap();
    let tampered = stamp::tamper_signature(&original).unwrap();
    f.execute(
        "UPDATE organization_identity SET organization_statement=?",
        vec![SqlValue::Text(tampered)],
    )
    .await;
    let reset: Result<bool, central_storage::StoreFailure> = runtime.control("crash", &()).await;
    assert!(reset.is_err());
    f.runtime().await;
    assert_eq!(f.post(DIGEST, &json!({"organizationId":ORG})).await.0, 503);
    assert_eq!(f.post(REVISIONS, &org_revision(1, "active")).await.0, 503);
    assert_eq!(
        f.read("SELECT id FROM organization_change_records")
            .await
            .rows
            .len(),
        1
    );
    f.close().await;
}
#[tokio::test]
async fn warm_key_refresh_boundary_rechecks_stored_and_incoming_stamps() {
    let f = Fixture::new().await;
    let first = stamp::stamp_under_second_key_at(ORG, NOW_SECONDS, 0).unwrap();
    assert_eq!(
        f.post(
            PROVISION,
            &json!({"organizationId":ORG,"organizationStatement":first})
        )
        .await
        .0,
        200
    );
    let warmed = f.keys.fetches();
    f.keys.retire("stamp-b").unwrap();
    f.clock
        .set_utc(chrono::DateTime::from_timestamp_millis(NOW_SECONDS * 1000 + 299_999).unwrap());
    f.runtime().await;
    let inside = stamp::stamp_under_second_key_at(ORG, NOW_SECONDS, 1).unwrap();
    assert_eq!(
        f.post(
            PROVISION,
            &json!({"organizationId":ORG,"organizationStatement":inside})
        )
        .await
        .0,
        200
    );
    let inside_fetches = f.keys.fetches();
    f.clock
        .set_utc(chrono::DateTime::from_timestamp_millis(NOW_SECONDS * 1000 + 300_000).unwrap());
    f.runtime().await;
    let outside = stamp::stamp_under_second_key_at(ORG, NOW_SECONDS, 2).unwrap();
    assert_eq!(
        f.post(
            PROVISION,
            &json!({"organizationId":ORG,"organizationStatement":outside})
        )
        .await
        .0,
        422
    );
    assert_eq!((warmed, inside_fetches, f.keys.fetches()), (1, 1, 3));
    let other = "019f0000-0000-7000-8000-00000000d299";
    f.host.central(&other.to_owned().try_into().unwrap()).await;
    assert_eq!(
        f.post(PROVISION, &json!({"organizationId":other,"organizationStatement":stamp::stamp(other,NOW_SECONDS).unwrap()})).await.0,
        200,
        "a fresh organization under the remaining key still provisions after the refresh bound"
    );
    let remaining = stamp::stamp(ORG, NOW_SECONDS).unwrap();
    assert_eq!(
        f.post(
            PROVISION,
            &json!({"organizationId":ORG,"organizationStatement":remaining})
        )
        .await
        .0,
        409
    );
    let rows = f
        .read("SELECT organization_statement FROM organization_identity")
        .await;
    assert!(matches!(&rows.rows[0][0],central_storage::SqlValue::Text(value)if value==&first));
    assert_eq!(
        f.read("SELECT id FROM organization_change_records")
            .await
            .rows
            .len(),
        1
    );
    f.close().await;
}
#[tokio::test]
async fn lost_provisioning_answer_survives_cold_reopen_and_exact_retry() {
    let f = Fixture::new().await;
    let runtime = f.runtime().await;
    let _: bool = runtime
        .control("lose-provisioning-reply", &())
        .await
        .unwrap();
    assert_eq!(f.provision().await.0, 503);
    let before=f.read("SELECT (SELECT json_group_array(json_array(id,organization_statement,created_at,updated_at)) FROM organization_identity),(SELECT json_group_array(json_array(id,request_id,kind,origin,target_id,created_at,updated_at)) FROM organization_change_records)").await;
    assert_eq!(
        f.read("SELECT id FROM organization_change_records")
            .await
            .rows
            .len(),
        1
    );
    let reset: Result<bool, central_storage::StoreFailure> = runtime.control("crash", &()).await;
    assert!(reset.is_err());
    f.runtime().await;
    assert_eq!(f.provision().await.0, 200);
    let after=f.read("SELECT (SELECT json_group_array(json_array(id,organization_statement,created_at,updated_at)) FROM organization_identity),(SELECT json_group_array(json_array(id,request_id,kind,origin,target_id,created_at,updated_at)) FROM organization_change_records)").await;
    assert!(
        after == before,
        "exact retry must preserve stamp/record identities and timestamps"
    );
    f.close().await;
}
#[tokio::test]
async fn competing_first_inputs_and_renewals_share_one_org_turn() {
    let f = Fixture::new().await;
    let first = stamp::stamp(ORG, NOW_SECONDS).unwrap();
    let alternative = stamp::first_statement(ORG, NOW_SECONDS, 1).unwrap();
    let first_body = json!({"organizationId":ORG,"organizationStatement":first});
    let alternative_body = json!({"organizationId":ORG,"organizationStatement":alternative});
    let (a, b) = tokio::join!(
        f.post(PROVISION, &first_body),
        f.post(PROVISION, &alternative_body)
    );
    assert_eq!((a.0, b.0), (200, 200));
    let retained = f.post(DIGEST, &json!({"organizationId":ORG})).await.1["digest"]
        .as_str()
        .unwrap()
        .to_owned();
    let predecessor = if retained == stamp::digest(&first) {
        &first
    } else {
        &alternative
    };
    let first_renewal = stamp::renewal(ORG, predecessor, NOW_SECONDS, 2).unwrap();
    let second_renewal = stamp::renewal(ORG, predecessor, NOW_SECONDS, 3).unwrap();
    let first_body = json!({"organizationId":ORG,"organizationStatement":first_renewal});
    let second_body = json!({"organizationId":ORG,"organizationStatement":second_renewal});
    let (a, b) = tokio::join!(
        f.post(PROVISION, &first_body),
        f.post(PROVISION, &second_body)
    );
    let mut statuses = [a.0, b.0];
    statuses.sort_unstable();
    assert_eq!(statuses, [200, 409]);
    assert_eq!(
        f.read("SELECT id FROM organization_identity")
            .await
            .rows
            .len(),
        1
    );
    assert_eq!(
        f.read("SELECT id FROM organization_change_records")
            .await
            .rows
            .len(),
        2
    );
    f.close().await;
}
#[tokio::test]
async fn provisioning_does_not_rewrite_existing_workspace_machine_or_credential_rows() {
    use central_storage::SqlValue;
    let f = Fixture::new().await;
    f.execute("INSERT INTO workspaces(workspace_id,name,state,created_at,updated_at) VALUES ('019f0000-0000-7000-8000-00000000d231','Existing','active','2030-03-17T17:46:40Z','2030-03-17T17:46:40Z')",vec![]).await;
    f.execute("INSERT INTO machines(machine_id,installation_id,label,state,allowed_workspaces,created_at,updated_at) VALUES ('019f0000-0000-7000-8000-00000000d232','019f0000-0000-7000-8000-00000000d233','Existing machine','enrolled','[]','2030-03-17T17:46:40Z','2030-03-17T17:46:40Z')",vec![]).await;
    f.execute("INSERT INTO machine_credentials(credential_id,machine_id,token_hash,expires_at,created_at,updated_at) VALUES ('019f0000-0000-7000-8000-00000000d234','019f0000-0000-7000-8000-00000000d232','fixture credential','2040-01-01T00:00:00Z','2030-03-17T17:46:40Z','2030-03-17T17:46:40Z')",vec![]).await;
    let workspaces = f.read("SELECT * FROM workspaces").await;
    let machines = f.read("SELECT * FROM machines").await;
    let credentials = f.read("SELECT * FROM machine_credentials").await;
    assert_eq!(f.provision().await.0, 200);
    assert_eq!(f.provision().await.0, 200);
    assert_eq!(f.read("SELECT * FROM workspaces").await, workspaces);
    assert_eq!(f.read("SELECT * FROM machines").await, machines);
    assert_eq!(
        f.read("SELECT * FROM machine_credentials").await,
        credentials
    );
    assert_eq!(
        f.counts().await.rows[0][0],
        SqlValue::Integer(1.try_into().unwrap())
    );
    f.close().await;
}

#[tokio::test]
async fn expired_after_cold_reset_and_foreign_stored_stamps_refuse_without_rewrite() {
    use central_storage::{ReadOutcome, SqlValue, Statement, StoreFailure};
    for foreign in [false, true] {
        let f = Fixture::new().await;
        let mut claims = stamp::claims(ORG, NOW_SECONDS);
        if !foreign {
            claims["exp"] = json!(NOW_SECONDS + 1);
        }
        let original = stamp::sign(&stamp::stamp_header(), &claims, 7).unwrap();
        assert_eq!(
            f.post(
                PROVISION,
                &json!({"organizationId":ORG,"organizationStatement":original})
            )
            .await
            .0,
            200
        );
        let runtime = f.runtime().await;
        if foreign {
            let other = "019f0000-0000-7000-8000-00000000d297";
            f.execute(
                "UPDATE organization_identity SET id=?,organization_statement=?",
                vec![
                    SqlValue::Text(other.to_owned()),
                    SqlValue::Text(stamp::stamp(other, NOW_SECONDS).unwrap()),
                ],
            )
            .await;
        } else {
            let reset: Result<bool, StoreFailure> = runtime.control("crash", &()).await;
            assert!(reset.is_err());
            f.clock.advance(std::time::Duration::from_secs(2)).unwrap();
            f.runtime().await;
        }
        let snapshot = || Statement {
            sql: "SELECT id,organization_statement,created_at,updated_at FROM organization_identity ORDER BY id",
            args: vec![],
        };
        let before: Result<ReadOutcome, StoreFailure> =
            runtime.control("raw-read", &snapshot()).await.unwrap();
        let before = before.unwrap();
        assert_eq!(
            f.post(DIGEST, &json!({"organizationId":ORG})).await,
            (503, serde_json::Value::Null)
        );
        assert_eq!(
            f.post(REVISIONS, &org_revision(1, "active")).await,
            (503, serde_json::Value::Null)
        );
        assert_eq!(f.records(&f.owner()).await, (503, serde_json::Value::Null));
        let after: Result<ReadOutcome, StoreFailure> =
            runtime.control("raw-read", &snapshot()).await.unwrap();
        assert!(
            after.unwrap() == before,
            "refused private calls must not replace the stored stamp"
        );
        f.close().await;
    }
}
