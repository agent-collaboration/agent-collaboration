//! F2: the ledger's 24-hour window does not expire directory identity or names.
use super::{owned_rows, support::*};
use central_storage::SqlValue;
use std::time::Duration;

async fn unchanged_directory(f: &Fixture) -> Vec<Vec<Vec<SqlValue>>> {
    let mut rows = vec![];
    for sql in [
        "SELECT * FROM directory_workspaces ORDER BY workspace_id",
        "SELECT * FROM signin_change_records ORDER BY id",
        "SELECT * FROM access_actor_identities ORDER BY actor_identity_id",
        "SELECT * FROM signin_organization_identity ORDER BY id",
        "SELECT * FROM signin_authorization_floors ORDER BY id",
        "SELECT * FROM signin_person_authorization_floors ORDER BY id",
    ] {
        rows.push(f.read(sql).await.rows);
    }
    rows
}
async fn ledger(f: &Fixture) -> Vec<Vec<SqlValue>> {
    f.read("SELECT caller,content_digest,answer,created_at,updated_at FROM runtime_request_log ORDER BY request_id").await.rows
}

#[tokio::test]
async fn admin_create_after_ledger_expiry_retains_name_refusal_and_rolls_back_identity_conflict() {
    // Isolated fixtures ensure the retained nameInUse answer never masks the
    // expired-ledger miss that must reach the directory's existing primary key.
    for changed_name in [false, true] {
        let f = ready().await;
        let owner = f.owner();
        let operation = operation(if changed_name { 0x7f2 } else { 0x7f1 });
        let (original_body, original_intent) =
            create(&operation, "Original workspace", "Original description");
        let first = f
            .post_bytes(
                signed::CREATE,
                &original_body,
                &headers(&owner, &original_intent, NOW_SECONDS),
            )
            .await;
        assert_eq!(first.0, 200);
        let workspace = json!({"workspaceId":operation,"name":"Original workspace","description":"Original description","state":"active"});
        assert_eq!(
            serde_json::from_slice::<Value>(&first.1).unwrap(),
            json!({"result":"applied","workspace":workspace})
        );
        let original = ledger(&f).await;
        assert_eq!(original.len(), 1);
        assert!(
            original.first().and_then(|row| row.get(2))
                == Some(&SqlValue::Text(String::from_utf8(first.1.clone()).unwrap())),
            "original Applied bytes must actually be retained"
        );
        let directory = unchanged_directory(&f).await;
        f.clock.advance(Duration::from_millis(86_400_001)).unwrap();
        f.runtime().await;
        let now = NOW_SECONDS * 1000 + 86_400_001;
        let now_seconds = now.div_euclid(1000);
        let now_text = chrono::DateTime::from_timestamp_millis(now)
            .unwrap()
            .to_rfc3339();
        let owner = signed::Person::owner(ORG, now_seconds).unwrap();
        let name = if changed_name {
            "Changed unused name"
        } else {
            "Original workspace"
        };
        let proof_id = signed::fresh_request_id(now_seconds).unwrap();
        let (body, intent) = create(&operation, name, "Original description");
        let proof = owner
            .headers(signed::CREATE, &intent, &proof_id, now_seconds)
            .unwrap();
        if changed_name {
            assert!(f.read("SELECT workspace_id FROM directory_workspaces WHERE lower(trim(name))=lower(trim('Changed unused name'))").await.rows.is_empty(),"the changed name must be free so only workspace_id can conflict");
        }
        let before = owned_rows(&f).await;
        let late = f.post_bytes(signed::CREATE, &body, &proof).await;
        assert_eq!(late.0, 200);
        if changed_name {
            assert_eq!(
                serde_json::from_slice::<Value>(&late.1).unwrap(),
                json!({"result":"refused","reason":"conflict"})
            );
            assert!(
                owned_rows(&f).await == before,
                "PK conflict partially committed an effect, marker or replacement answer"
            );
            assert!(
                ledger(&f).await == original,
                "unretained PK conflict replaced the original Applied answer"
            );
            let retry = f.post_bytes(signed::CREATE, &body, &proof).await;
            assert_eq!(retry.0, 200);
            assert!(
                retry.1 == late.1,
                "same proof must conflict again rather than requestReused"
            );
            assert!(
                owned_rows(&f).await == before,
                "same-proof PK retry consumed or changed state"
            );
        } else {
            assert_eq!(
                serde_json::from_slice::<Value>(&late.1).unwrap(),
                json!({"result":"refused","reason":"nameInUse"})
            );
            let refusal = ledger(&f).await;
            assert_eq!(refusal.len(), 1);
            let row = refusal.first().unwrap();
            let old = original.first().unwrap();
            assert!(
                row.first() == old.first() && row.get(1) == old.get(1),
                "same-name late decision preserves actor/content identity"
            );
            assert_eq!(
                row.get(2),
                Some(&SqlValue::Text(
                    "{\"result\":\"refused\",\"reason\":\"nameInUse\"}".into()
                ))
            );
            assert_eq!(row.get(3), Some(&SqlValue::Text(now_text.clone())));
            assert_eq!(row.get(4), Some(&SqlValue::Text(now_text)));
            assert!(
                refusal != original,
                "late same-name retry must retain a new refusal, not old Applied"
            );
            assert!(
                unchanged_directory(&f).await == directory,
                "late name refusal changed directory, audit, actor or authority"
            );
            assert_eq!(
                f.read("SELECT COUNT(*) FROM admin_used_requests")
                    .await
                    .rows,
                vec![vec![number(2)]]
            );
            let reused = f.post_bytes(signed::CREATE, &body, &proof).await;
            assert_eq!(reused.0, 200);
            assert_eq!(
                serde_json::from_slice::<Value>(&reused.1).unwrap(),
                json!({"result":"refused","reason":"requestReused"})
            );
            let replay = f
                .post_bytes(
                    signed::CREATE,
                    &body,
                    &headers(&owner, &intent, now_seconds),
                )
                .await;
            assert_eq!(replay.0, 200);
            assert!(
                replay.1 == late.1,
                "new proof must replay the newly retained refusal bytes"
            );
            assert!(
                ledger(&f).await == refusal,
                "refusal replay must preserve its new timestamp and answer"
            );
        }
        // Lookup is the consumer's recovery for its own operation ID. On the
        // PK path, successfully reserving this prior failed request's jti also
        // proves both conflict attempts left the proof marker unconsumed.
        let lookup_id = if changed_name {
            proof_id
        } else {
            signed::fresh_request_id(now_seconds).unwrap()
        };
        let lookup_proof = owner
            .headers(
                LOOKUP,
                &signed::lookup_intent(ORG, &operation),
                &lookup_id,
                now_seconds,
            )
            .unwrap();
        let lookup = f
            .post_with(
                LOOKUP,
                &json!({"organizationId":ORG,"workspaceId":operation}),
                &lookup_proof,
            )
            .await;
        assert_eq!(
            lookup,
            (200, json!({"result":"found","workspace":workspace}))
        );
        assert!(
            unchanged_directory(&f).await == directory,
            "late retries or lookup changed the original workspace/audit"
        );
        if changed_name {
            assert!(ledger(&f).await == original);
        }
        f.close().await;
    }
}
