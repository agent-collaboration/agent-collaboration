use super::support::*;
use serde_json::json;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Real signed requests and unchanged owning-row assertions propagate fixture failures."
)]
#[tokio::test]
async fn create_replay_requires_current_permission_and_live_floor() -> Result {
    let f = Fixture::new().await?;
    f.ready().await?;
    let owner = signed::Person::owner(A, signed::NOW_SECONDS)?;
    let operation = "019f0000-0000-7000-8000-00000000d711";
    assert_eq!(
        create(&f, &owner, operation, "Replay permission", "")
            .await?
            .status,
        200
    );
    let before = all_effects(&f).await?;
    let member = signed::Person::member(A, signed::NOW_SECONDS)?;
    let denied = create(&f, &member, operation, "Replay permission", "").await?;
    assert_eq!(denied.status, 200);
    assert_eq!(
        denied.value()?,
        json!({"result":"refused","reason":"notPermitted"})
    );
    assert!(
        all_effects(&f).await? == before,
        "unauthorized retry revealed or mutated retained state"
    );
    assert_eq!(
        f.post(
            REVISIONS,
            &person_floor(A, signed::OWNER, signed::OWNER_SUBJECT, 5, "inactive")
        )
        .await?
        .status,
        200
    );
    let before = all_effects(&f).await?;
    let denied = create(&f, &owner, operation, "Replay permission", "").await?;
    assert_eq!(denied.status, 200);
    assert_eq!(
        denied.value()?,
        json!({"result":"refused","reason":"notPermitted"})
    );
    assert!(
        all_effects(&f).await? == before,
        "revoked owner retry wrote or reserved something"
    );
    f.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The distinct authorized caller must conflict without replacing the retained answer."
)]
#[tokio::test]
async fn different_authorized_person_cannot_reuse_create_operation() -> Result {
    let f = Fixture::new().await?;
    f.ready().await?;
    let owner = signed::Person::owner(A, signed::NOW_SECONDS)?;
    let operation = "019f0000-0000-7000-8000-00000000d721";
    let first = create(&f, &owner, operation, "First actor", "").await?;
    assert_eq!(first.status, 200);
    let before = durable_effects(&f).await?;
    let mut other = signed::Person::member(A, signed::NOW_SECONDS)?;
    other.permissions = vec!["workspace:create".to_owned()];
    let conflict = create(&f, &other, operation, "First actor", "").await?;
    assert_eq!(conflict.status, 200);
    assert_eq!(
        conflict.value()?,
        json!({"result":"refused","reason":"conflict"})
    );
    assert!(
        durable_effects(&f).await? == before,
        "other actor changed directory, record, ledger or actor map"
    );
    assert_eq!(
        f.rows(A, "SELECT COUNT(*) FROM organization_used_requests", vec![])
            .await?,
        vec![vec![number(2)?]],
        "admitted conflicting operation consumes only its fresh proof"
    );
    assert_eq!(
        create(&f, &owner, operation, "First actor", "").await?.body,
        first.body
    );
    f.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Tampered signed intent must fail at the actual verifier before any persistent admission."
)]
#[tokio::test]
async fn signed_create_body_tamper_refuses_before_admission() -> Result {
    let f = Fixture::new().await?;
    f.ready().await?;
    let owner = signed::Person::owner(A, signed::NOW_SECONDS)?;
    let operation = "019f0000-0000-7000-8000-00000000d731";
    let before = all_effects(&f).await?;
    let headers = owner.headers(
        signed::CREATE,
        &intent(operation, "Signed name", "description"),
        &signed::fresh_request_id(signed::NOW_SECONDS)?,
        signed::NOW_SECONDS,
    )?;
    let tampered = post(
        &f.client(),
        signed::CREATE,
        serde_json::to_string(&body(operation, "Changed name", "description"))?,
        headers,
    )
    .await?;
    assert_eq!(tampered.status, 401);
    assert!(tampered.body.is_empty());
    assert!(
        all_effects(&f).await? == before,
        "tampered body reserved a proof or changed owning rows"
    );
    let valid = create(&f, &owner, operation, "Signed name", "description").await?;
    assert_eq!(valid.status, 200);
    assert_eq!(valid.value()?.get("result"), Some(&json!("applied")));
    f.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Literal audience-array failure and real Create control retain the original contract."
)]
#[tokio::test]
async fn create_audience_array_requires_admin_and_applies_control() -> Result {
    let f = Fixture::new().await?;
    f.ready().await?;
    let owner = signed::Person::owner(A, signed::NOW_SECONDS)?;
    for (operation, audience, status) in [
        (
            "019f0000-0000-7000-8000-00000000d741",
            json!(["https://other.example.invalid/admin"]),
            401,
        ),
        (
            "019f0000-0000-7000-8000-00000000d742",
            json!([
                "https://other.example.invalid/admin",
                signed::ADMIN_AUDIENCE
            ]),
            200,
        ),
    ] {
        let before = all_effects(&f).await?;
        let headers = owner.headers_with(
            signed::CREATE,
            &intent(operation, operation, ""),
            &signed::fresh_request_id(signed::NOW_SECONDS)?,
            signed::NOW_SECONDS,
            &|claims| {
                if let Some(claims) = claims.as_object_mut() {
                    claims.insert("aud".into(), audience.clone());
                }
            },
        )?;
        let answer = post(
            &f.client(),
            signed::CREATE,
            serde_json::to_string(&body(operation, operation, ""))?,
            headers,
        )
        .await?;
        assert_eq!(answer.status, status);
        if status == 401 {
            assert!(answer.body.is_empty());
            assert!(
                all_effects(&f).await? == before,
                "audience refusal reserved or wrote"
            );
        } else {
            assert_eq!(answer.value()?.get("result"), Some(&json!("applied")));
        }
    }
    f.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Real Create permission, factor and DPoP downgrade refusals must preserve every owning row."
)]
#[tokio::test]
async fn create_permission_factor_bearer_missing_proof_and_wrong_target_write_nothing() -> Result {
    let f = Fixture::new().await?;
    f.ready().await?;
    let owner = signed::Person::owner(A, signed::NOW_SECONDS)?;
    let operation = "019f0000-0000-7000-8000-00000000d751";
    let before = all_effects(&f).await?;
    let member = signed::Person::member(A, signed::NOW_SECONDS)?;
    let denied = create(&f, &member, operation, "Guarded", "").await?;
    assert_eq!(denied.status, 200);
    assert_eq!(
        denied.value()?,
        json!({"result":"refused","reason":"notPermitted"})
    );
    assert!(
        all_effects(&f).await? == before,
        "member without create permission changed owning rows"
    );
    let mut stale = signed::Person::owner(A, signed::NOW_SECONDS)?;
    stale.second_factor_at = Some(signed::NOW_SECONDS - 901);
    let mut absent = signed::Person::owner(A, signed::NOW_SECONDS)?;
    absent.second_factor_at = None;
    for person in [&stale, &absent] {
        let reply = create(&f, person, operation, "Guarded", "").await?;
        assert_eq!(reply.status, 200);
        assert_eq!(
            reply.value()?,
            json!({"result":"refused","reason":"secondFactorRequired"})
        );
        assert!(all_effects(&f).await? == before);
    }
    for kind in ["bearer", "no proof", "wrong target"] {
        let mut headers = owner.headers(
            if kind == "wrong target" {
                signed::LOOKUP
            } else {
                signed::CREATE
            },
            &intent(operation, "Guarded", ""),
            &signed::fresh_request_id(signed::NOW_SECONDS)?,
            signed::NOW_SECONDS,
        )?;
        if kind == "bearer" {
            let first = headers.first_mut().ok_or("authorization missing")?;
            first.1 = first.1.replacen("DPoP ", "Bearer ", 1);
        }
        if kind == "no proof" {
            headers.retain(|(name, _)| *name != "dpop");
        }
        let reply = post(
            &f.client(),
            signed::CREATE,
            serde_json::to_string(&body(operation, "Guarded", ""))?,
            headers,
        )
        .await?;
        assert_eq!(reply.status, 401, "{kind}");
        assert!(reply.body.is_empty());
        assert!(
            all_effects(&f).await? == before,
            "{kind} changed owning rows"
        );
    }
    let applied = create(&f, &owner, operation, "Guarded", "").await?;
    assert_eq!(applied.status, 200);
    assert_eq!(applied.value()?.get("result"), Some(&json!("applied")));
    f.close().await
}
