use super::support::*;
use serde_json::{Value, json};

#[expect(
    clippy::panic_in_result_fn,
    reason = "Three real provisioning records provide independent request identities for exclusive-cursor page assertions."
)]
#[tokio::test]
async fn audit_pages_follow_three_committed_records_once_and_end_after_the_last_id() -> Result {
    let f = Fixture::new().await?;
    f.ready().await?;
    let first = signed::stamp(A, signed::NOW_SECONDS)?;
    let renewed = signed::renewal(A, &first, signed::NOW_SECONDS, 10)?;
    let renewed_again = signed::renewal(A, &renewed, signed::NOW_SECONDS, 20)?;
    assert_eq!(f.provision(A, &renewed).await?.status, 200);
    assert_eq!(f.provision(A, &renewed_again).await?.status, 200);
    let owner = signed::Person::owner(A, signed::NOW_SECONDS)?;
    let mut after: Option<String> = None;
    let mut entries = Vec::new();
    for _ in 0..3 {
        let body = json!({"organizationId":A,"after":after,"limit":1});
        let answer = f
            .person(
                signed::RECORDS,
                &body,
                &owner,
                &signed::records_intent(A, after.as_deref(), 1),
                &signed::fresh_request_id(signed::NOW_SECONDS)?,
            )
            .await?;
        assert_eq!(answer.status, 200);
        let answer = answer.value()?;
        assert_eq!(answer.get("result"), Some(&json!("records")));
        let records = answer
            .get("records")
            .and_then(Value::as_array)
            .ok_or("record page missing")?;
        assert_eq!(
            records.len(),
            1,
            "limit=1 returns exactly one remaining record"
        );
        let record = records.first().ok_or("record missing")?.clone();
        after = Some(
            record
                .get("id")
                .and_then(Value::as_str)
                .ok_or("record id missing")?
                .to_owned(),
        );
        entries.push(record);
    }
    let request_ids = entries
        .iter()
        .map(|entry| {
            entry
                .get("requestId")
                .and_then(Value::as_str)
                .ok_or("request id missing")
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let renewed_digest = signed::digest(&renewed);
    let renewed_again_digest = signed::digest(&renewed_again);
    assert_eq!(
        request_ids,
        vec![A, renewed_digest.as_str(), renewed_again_digest.as_str()]
    );
    let kinds = entries
        .iter()
        .map(|entry| {
            entry
                .get("kind")
                .and_then(Value::as_str)
                .ok_or("record kind missing")
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    assert_eq!(
        kinds,
        vec!["organizationProvisioned", "stampRenewed", "stampRenewed"]
    );
    let ids = entries
        .iter()
        .map(|entry| entry.get("id").and_then(Value::as_str).ok_or("id missing"))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    assert!(
        ids.windows(2)
            .all(|pair| matches!(pair,[before,after]if before<after)),
        "the exclusive cursor follows strict id order without duplicates"
    );
    assert_eq!(
        ids.iter().collect::<std::collections::BTreeSet<_>>().len(),
        3
    );
    let last = f
        .person(
            signed::RECORDS,
            &json!({"organizationId":A,"after":after,"limit":1}),
            &owner,
            &signed::records_intent(A, after.as_deref(), 1),
            &signed::fresh_request_id(signed::NOW_SECONDS)?,
        )
        .await?;
    assert_eq!(last.status, 200);
    assert_eq!(last.value()?, json!({"result":"records","records":[]}));
    assert_eq!(
        f.snapshot().await?.1.len(),
        3,
        "pagination adds no change record"
    );
    f.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The existing production public TCP server must never expose the private Unix link's paths."
)]
#[tokio::test]
async fn real_public_tcp_excludes_all_private_paths_before_and_after_owner_credentials() -> Result {
    use collaboration_central_tokio::{TokioServer, TokioSettings};
    use std::time::Duration;
    let f = Fixture::new().await?;
    let statement = signed::stamp(A, signed::NOW_SECONDS)?;
    assert_eq!(
        f.provision(A, &statement).await?.status,
        200,
        "the dedicated Unix link positively mounts provisioning"
    );
    f.ready().await?;
    let before = f.snapshot().await?;
    let rows_to_watch = [
        "SELECT * FROM workspaces ORDER BY workspace_id",
        "SELECT * FROM request_log ORDER BY request_id",
        "SELECT * FROM organization_used_requests ORDER BY id",
        "SELECT * FROM actor_identities ORDER BY actor_identity_id",
    ];
    let mut before_admission = Vec::new();
    for sql in rows_to_watch {
        before_admission.push(f.rows(A, sql, vec![]).await?);
    }
    let server = TokioServer::start_with_clock(
        TokioSettings {
            root: f.root.path().join("public"),
            access_policy: collaboration_central_conformance::fixture_policy(),
            jwks_url: f.keys.url.clone(),
            registry: crate::configuration::registry_config()?,
            delivery: collaboration_central_tokio::DeliveryConfig {
                relay_peer: "019f0000-0000-7000-8000-000000005240"
                    .to_owned()
                    .try_into()?,
            },
            holder_answer_timeout: Duration::from_secs(2),
            copy_ack_wait: Duration::from_secs(2),
        },
        f.clock.clone(),
    )
    .await?;
    let client = reqwest::Client::builder().no_proxy().build()?;
    let owner = collaboration_central_conformance::owner_assertion()?;
    // Lead settlement 01a1d910-44a9 retains the current public 401 behavior,
    // superseding B2's old outer 404 guard. A supplied owner assertion reaches
    // RouteRefused; no assertion retains MissingAssertion. Unknown paths on the
    // separate private link still return 404.
    for path in [
        PROVISION,
        DIGEST,
        REVISIONS,
        signed::CREATE,
        signed::LOOKUP,
        signed::RECORDS,
    ] {
        for authorized in [false, true] {
            let operation = "019f0000-0000-7000-8000-00000000d7e1";
            let body = if path == signed::CREATE {
                signed::create_body(A, operation, "Public create must refuse")
            } else {
                provision_body(A, &statement)
            };
            let mut request = client
                .post(format!("{}{path}", server.base_url()))
                .header("content-type", "application/json")
                .body(serde_json::to_string(&body)?);
            if path == signed::CREATE {
                let person = signed::Person::owner(A, signed::NOW_SECONDS)?;
                for (name, value) in person.headers(
                    signed::CREATE,
                    &signed::create_intent(A, operation, "Public create must refuse"),
                    &signed::fresh_request_id(signed::NOW_SECONDS)?,
                    signed::NOW_SECONDS,
                )? {
                    request = request.header(name, value);
                }
            }
            if authorized {
                request = request.header("Cf-Access-Jwt-Assertion", &owner);
            }
            let response = request.send().await?;
            assert_eq!(
                response.status().as_u16(),
                401,
                "public path {path}, owner credential supplied: {authorized}"
            );
            let body: Value = response.json().await?;
            assert_eq!(
                body,
                json!({"kind":if authorized{"routeRefused"}else{"missingAssertion"}})
            );
        }
    }
    server.shutdown().await?;
    assert!(
        f.snapshot().await? == before,
        "public requests changed no modern identity or record"
    );
    let mut after_admission = Vec::new();
    for sql in rows_to_watch {
        after_admission.push(f.rows(A, sql, vec![]).await?);
    }
    assert!(
        after_admission == before_admission,
        "public refusal changed directory, ledger, proof or actor rows"
    );
    f.close().await
}
