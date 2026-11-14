use super::support::*;
use serde_json::json;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Real Ed25519 person tokens and P-256 proofs determine all admission outcomes."
)]
#[tokio::test]
async fn signed_records_unknown_lookup_fresh_proof_replay_intent_and_actor_permissions() -> Result {
    let f = Fixture::new().await?;
    f.ready().await?;
    let mut owner = signed::Person::owner(A, signed::NOW_SECONDS)?;
    let records = json!({"organizationId":A,"after":null,"limit":100});
    let intent = signed::records_intent(A, None, 100);
    let request_id = signed::fresh_request_id(signed::NOW_SECONDS)?;
    let first = f
        .person(signed::RECORDS, &records, &owner, &intent, &request_id)
        .await?;
    assert_eq!(first.status, 200);
    let result = first.value()?;
    assert_eq!(result.get("result"), Some(&json!("records")));
    let entries = result
        .get("records")
        .and_then(|value| value.as_array())
        .ok_or("records missing")?;
    assert_eq!(entries.len(), 1);
    let entry = entries.first().ok_or("first record missing")?;
    assert_eq!(entry.get("kind"), Some(&json!("organizationProvisioned")));
    assert_eq!(
        entry.get("origin"),
        Some(&json!({"kind":"signInProvisioning"}))
    );
    assert_eq!(entry.get("targetId"), Some(&json!(A)));
    assert_eq!(
        f.person(signed::RECORDS, &records, &owner, &intent, &request_id)
            .await?
            .value()?,
        json!({"result":"refused","reason":"requestReused"})
    );
    assert_eq!(
        f.person(
            signed::RECORDS,
            &records,
            &owner,
            &intent,
            &signed::fresh_request_id(signed::NOW_SECONDS)?
        )
        .await?
        .status,
        200,
        "fresh proof may retry the same read"
    );
    let after = entry
        .get("id")
        .and_then(|value| value.as_str())
        .ok_or("record id missing")?;
    let page = json!({"organizationId":A,"after":after,"limit":1});
    assert_eq!(
        f.person(
            signed::RECORDS,
            &page,
            &owner,
            &signed::records_intent(A, Some(after), 1),
            &signed::fresh_request_id(signed::NOW_SECONDS)?
        )
        .await?
        .value()?,
        json!({"result":"records","records":[]})
    );
    owner.second_factor_at = None; // These read routes require no change-only second factor.
    let lookup = json!({"organizationId":A,"workspaceId":WORKSPACE});
    assert_eq!(
        f.person(
            signed::LOOKUP,
            &lookup,
            &owner,
            &signed::lookup_intent(A, WORKSPACE),
            &signed::fresh_request_id(signed::NOW_SECONDS)?
        )
        .await?
        .value()?,
        json!({"result":"found","workspace":null})
    );
    let marker = f
        .rows(
            A,
            "SELECT actor_person_id,actor_session_id FROM organization_used_requests WHERE id=?",
            vec![text(&request_id)],
        )
        .await?;
    assert_eq!(
        marker,
        vec![vec![text(signed::OWNER), text(signed::SESSION)]],
        "fresh proof records its actual person and session"
    );
    let before = f
        .rows(
            A,
            "SELECT id FROM organization_used_requests ORDER BY id",
            vec![],
        )
        .await?;
    let changed = json!({"organizationId":A,"after":null,"limit":50});
    let tampered = f
        .person(
            signed::RECORDS,
            &changed,
            &owner,
            &intent,
            &signed::fresh_request_id(signed::NOW_SECONDS)?,
        )
        .await?;
    assert_eq!(tampered.status, 401);
    assert!(tampered.body.is_empty());
    assert_eq!(f.post(signed::RECORDS, &records).await?.status, 401);
    let member = signed::Person::member(A, signed::NOW_SECONDS)?;
    assert_eq!(
        f.person(
            signed::RECORDS,
            &records,
            &member,
            &intent,
            &signed::fresh_request_id(signed::NOW_SECONDS)?
        )
        .await?
        .value()?,
        json!({"result":"refused","reason":"notPermitted"})
    );
    assert_eq!(
        f.rows(
            A,
            "SELECT id FROM organization_used_requests ORDER BY id",
            vec![]
        )
        .await?,
        before,
        "unauthorized or tampered calls reserve no proof id"
    );
    let headers = owner.headers_with(
        signed::RECORDS,
        &intent,
        &signed::fresh_request_id(signed::NOW_SECONDS)?,
        signed::NOW_SECONDS,
        &|claims| {
            if let Some(object) = claims.as_object_mut() {
                object.insert("aud".into(), json!(["unrelated-audience"]));
            }
        },
    )?;
    assert_eq!(
        post(
            &f.client(),
            signed::RECORDS,
            serde_json::to_string(&records)?,
            headers
        )
        .await?
        .status,
        401
    );
    f.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Token admission keeps refreshRequired distinct from authorizationSyncRequired."
)]
#[tokio::test]
async fn token_floor_relations_and_current_actor_revocation_refuse_before_a_proof_marker() -> Result
{
    let f = Fixture::new().await?;
    f.ready().await?;
    let records = json!({"organizationId":A,"after":null,"limit":100});
    let intent = signed::records_intent(A, None, 100);
    for (person_revision, organization_revision, reason) in [
        (4, 2, "refreshRequired"),
        (4, 4, "authorizationSyncRequired"),
        (3, 3, "refreshRequired"),
        (5, 3, "authorizationSyncRequired"),
    ] {
        let mut person = signed::Person::owner(A, signed::NOW_SECONDS)?;
        person.revision = person_revision;
        person.organization_revision = organization_revision;
        let reply = f
            .person(
                signed::RECORDS,
                &records,
                &person,
                &intent,
                &signed::fresh_request_id(signed::NOW_SECONDS)?,
            )
            .await?;
        assert_eq!(reply.status, 200);
        assert_eq!(reply.value()?, json!({"result":"refused","reason":reason}));
    }
    let mut owner = signed::Person::owner(A, signed::NOW_SECONDS)?;
    assert_eq!(
        f.post(
            REVISIONS,
            &person_floor(A, signed::OWNER, signed::OWNER_SUBJECT, 5, "inactive")
        )
        .await?
        .status,
        200
    );
    owner.revision = 5;
    assert_eq!(
        f.person(
            signed::RECORDS,
            &records,
            &owner,
            &intent,
            &signed::fresh_request_id(signed::NOW_SECONDS)?
        )
        .await?
        .value()?,
        json!({"result":"refused","reason":"notPermitted"})
    );
    assert!(
        f.rows(A, "SELECT id FROM organization_used_requests", vec![])
            .await?
            .is_empty()
    );
    f.close().await
}
