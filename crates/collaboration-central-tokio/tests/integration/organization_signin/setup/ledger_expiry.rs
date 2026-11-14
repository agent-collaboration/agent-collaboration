//! F2: operation replay expires; the workspace identity and unique name do not.
use super::support::*;
use central_storage::SqlValue;
use serde_json::json;
use std::time::Duration;

async fn unchanged_directory(f: &Fixture) -> Result<Vec<Vec<Vec<SqlValue>>>> {
    let mut rows = vec![];
    for sql in [
        "SELECT * FROM workspaces ORDER BY workspace_id",
        "SELECT * FROM organization_change_records ORDER BY id",
        "SELECT * FROM actor_identities ORDER BY actor_identity_id",
        "SELECT * FROM organization_identity ORDER BY id",
        "SELECT * FROM organization_authorization_floors ORDER BY id",
        "SELECT * FROM person_organization_authorization_floors ORDER BY id",
    ] {
        rows.push(f.rows(A, sql, vec![]).await?);
    }
    Ok(rows)
}

async fn ledger(f: &Fixture, operation: &str) -> Result<Vec<Vec<SqlValue>>> {
    f.rows(A,"SELECT caller,content_digest,answer,created_at,updated_at FROM request_log WHERE request_id=?",vec![text(operation)]).await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Independent late retries must expose the exact retained refusal or fully rolled-back PK conflict over real signed HTTP."
)]
#[tokio::test]
async fn admin_create_after_ledger_expiry_retains_name_refusal_and_rolls_back_identity_conflict()
-> Result {
    // Each path owns its original Applied ledger row. The same-name refusal
    // cannot replace the row before the changed-name PK path is exercised.
    for changed_name in [false, true] {
        let f = Fixture::new().await?;
        f.ready().await?;
        let owner = signed::Person::owner(A, signed::NOW_SECONDS)?;
        let operation = if changed_name {
            "019f0000-0000-7000-8000-00000000d7f2"
        } else {
            "019f0000-0000-7000-8000-00000000d7f1"
        };
        let first = create(
            &f,
            &owner,
            operation,
            "Original workspace",
            "Original description",
        )
        .await?;
        assert_eq!(first.status, 200);
        let workspace = json!({"workspaceId":operation,"name":"Original workspace","description":"Original description","state":"active"});
        assert_eq!(
            first.value()?,
            json!({"result":"applied","workspace":workspace})
        );
        let original = ledger(&f, operation).await?;
        assert_eq!(original.len(), 1);
        assert!(
            original.first().and_then(|row| row.get(2))
                == Some(&text(String::from_utf8(first.body.clone())?)),
            "initial ledger must hold the original Applied answer bytes"
        );
        let directory = unchanged_directory(&f).await?;
        f.clock.advance(Duration::from_millis(86_400_001))?;
        let now = signed::NOW_SECONDS * 1000 + 86_400_001;
        let now_text = chrono::DateTime::from_timestamp_millis(now)
            .ok_or("fixture UTC range")?
            .to_rfc3339();
        let owner = signed::Person::owner(A, f.clock_time())?;
        let name = if changed_name {
            "Changed unused name"
        } else {
            "Original workspace"
        };
        let proof_id = signed::fresh_request_id(f.clock_time())?;
        let request = body(operation, name, "Original description");
        let headers = owner.headers(
            signed::CREATE,
            &intent(operation, name, "Original description"),
            &proof_id,
            f.clock_time(),
        )?;
        if changed_name {
            assert!(
                f.rows(
                    A,
                    "SELECT workspace_id FROM workspaces WHERE lower(trim(name))=lower(trim(?))",
                    vec![text(name)]
                )
                .await?
                .is_empty(),
                "changed-name path must reach the existing workspace PK, not name collision"
            );
        }
        let before = all_effects(&f).await?;
        let late = post(
            &f.client(),
            signed::CREATE,
            serde_json::to_string(&request)?,
            headers.clone(),
        )
        .await?;
        assert_eq!(late.status, 200);
        if changed_name {
            assert_eq!(
                late.value()?,
                json!({"result":"refused","reason":"conflict"})
            );
            assert!(
                all_effects(&f).await? == before,
                "PK conflict retained an answer, proof or partial effect"
            );
            assert!(
                ledger(&f, operation).await? == original,
                "unretained PK conflict replaced the original Applied ledger row"
            );
            let retried = post(
                &f.client(),
                signed::CREATE,
                serde_json::to_string(&request)?,
                headers,
            )
            .await?;
            assert_eq!(retried.status, 200);
            assert_eq!(
                retried.body, late.body,
                "the identical unconsumed proof must conflict again, never requestReused"
            );
            assert!(
                all_effects(&f).await? == before,
                "same-proof PK retry changed owning rows"
            );
        } else {
            assert_eq!(
                late.value()?,
                json!({"result":"refused","reason":"nameInUse"})
            );
            let refusal = ledger(&f, operation).await?;
            let row = refusal.first().ok_or("new refusal ledger row missing")?;
            let old = original.first().ok_or("original ledger row missing")?;
            assert_eq!(refusal.len(), 1);
            assert!(
                row.first() == old.first() && row.get(1) == old.get(1),
                "late same-name decision changes neither actor nor content identity"
            );
            assert_eq!(
                row.get(2),
                Some(&text("{\"result\":\"refused\",\"reason\":\"nameInUse\"}"))
            );
            assert_eq!(row.get(3), Some(&text(&now_text)));
            assert_eq!(row.get(4), Some(&text(&now_text)));
            assert!(
                refusal != original,
                "the expired Applied answer was not replaced by a newly retained refusal"
            );
            assert!(
                unchanged_directory(&f).await? == directory,
                "late name refusal changed workspace, audit, actor or authority"
            );
            assert_eq!(
                f.rows(A, "SELECT COUNT(*) FROM organization_used_requests", vec![])
                    .await?,
                vec![vec![number(2)?]]
            );
            let reused = post(
                &f.client(),
                signed::CREATE,
                serde_json::to_string(&request)?,
                headers,
            )
            .await?;
            assert_eq!(reused.status, 200);
            assert_eq!(
                reused.value()?,
                json!({"result":"refused","reason":"requestReused"})
            );
            let replay = create(
                &f,
                &owner,
                operation,
                "Original workspace",
                "Original description",
            )
            .await?;
            assert_eq!(replay.status, 200);
            assert_eq!(
                replay.body, late.body,
                "fresh proof replays the newly retained refusal bytes"
            );
            assert!(
                ledger(&f, operation).await? == refusal,
                "refusal replay rewrote the new decision timestamp"
            );
        }
        // A late same-name caller can recover its real workspace by operation ID.
        // On the PK path reuse the previously unconsumed jti for this new signed
        // lookup intent: success proves the failed create consumed no marker.
        let lookup_id = if changed_name {
            proof_id
        } else {
            signed::fresh_request_id(f.clock_time())?
        };
        let lookup = f
            .person(
                signed::LOOKUP,
                &json!({"organizationId":A,"workspaceId":operation}),
                &owner,
                &signed::lookup_intent(A, operation),
                &lookup_id,
            )
            .await?;
        assert_eq!(lookup.status, 200);
        assert_eq!(
            lookup.value()?,
            json!({"result":"found","workspace":workspace})
        );
        assert!(
            unchanged_directory(&f).await? == directory,
            "lookup or late retry changed the original directory or audit"
        );
        if changed_name {
            assert!(ledger(&f, operation).await? == original);
        }
        f.close().await?;
    }
    Ok(())
}
