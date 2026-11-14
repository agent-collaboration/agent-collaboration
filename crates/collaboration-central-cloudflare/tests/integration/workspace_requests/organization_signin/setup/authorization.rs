use super::{owned_rows, support::*};

#[tokio::test]
async fn same_actor_changed_content_cannot_reuse_a_retained_create_operation() {
    let f = ready().await;
    let owner = f.owner();
    let operation = operation(0x727);
    let first = submit(&f, &owner, &operation, "First content").await;
    assert_eq!(first.0, 200);
    assert_eq!(first.1["result"], "applied");
    let before = stable_effects(&f).await;
    let before_markers = f
        .read("SELECT COUNT(*) FROM organization_used_requests")
        .await
        .rows;
    // This is a new valid proof over the changed body and intent, by the same
    // currently authorized actor. Verification succeeds; retained content conflicts.
    let changed = submit(&f, &owner, &operation, "Different content").await;
    assert_eq!(
        changed,
        (200, json!({"result":"refused","reason":"conflict"}))
    );
    assert!(
        stable_effects(&f).await == before,
        "changed content must not rewrite the retained directory, ledger, actor or audit"
    );
    assert_eq!(before_markers, vec![vec![number(1)]]);
    assert_eq!(
        f.read("SELECT COUNT(*) FROM organization_used_requests")
            .await
            .rows,
        vec![vec![number(2)]],
        "the admitted conflict reserves only its new valid proof"
    );
    f.close().await;
}

#[tokio::test]
async fn create_replay_requires_current_permission_and_live_floor() {
    let f = ready().await;
    member_floor(&f).await;
    let owner = f.owner();
    let operation = operation(0x721);
    assert_eq!(
        submit(&f, &owner, &operation, "Retained").await.1["result"],
        "applied"
    );
    let before = owned_rows(&f).await;
    let member = signed::Person::member(ORG, NOW_SECONDS).unwrap();
    assert_eq!(
        submit(&f, &member, &operation, "Retained").await,
        (200, json!({"result":"refused","reason":"notPermitted"}))
    );
    assert!(
        owned_rows(&f).await == before,
        "member retry must reveal no retained answer and write nothing"
    );
    assert_eq!(
        f.post(REVISIONS, &person_revision(5, "inactive")).await.0,
        200
    );
    assert_eq!(
        submit(&f, &owner, &operation, "Retained").await,
        (200, json!({"result":"refused","reason":"notPermitted"}))
    );
    assert!(
        owned_rows(&f).await == before,
        "revoked owner retry must reveal no retained answer and write nothing"
    );
    f.close().await;
}

#[tokio::test]
async fn different_authorized_person_cannot_reuse_create_operation() {
    let f = ready().await;
    member_floor(&f).await;
    let operation = operation(0x722);
    let first = submit(&f, &f.owner(), &operation, "First actor").await;
    assert_eq!(first.1["result"], "applied");
    let before = stable_effects(&f).await;
    let mut member = signed::Person::member(ORG, NOW_SECONDS).unwrap();
    member.permissions.push("workspace:create".into());
    assert_eq!(
        submit(&f, &member, &operation, "First actor").await,
        (200, json!({"result":"refused","reason":"conflict"}))
    );
    assert!(
        stable_effects(&f).await == before,
        "different actor must create no directory, ledger, actor or change row"
    );
    f.close().await;
}

#[tokio::test]
async fn signed_create_body_tamper_refuses_before_admission() {
    let f = ready().await;
    let owner = f.owner();
    let operation = operation(0x723);
    let (body, intent) = create(&operation, "Signed", "");
    let mut changed = body.clone();
    changed["name"] = json!("Changed");
    let before = owned_rows(&f).await;
    let refused = f
        .post_bytes(
            signed::CREATE,
            &changed,
            &headers(&owner, &intent, NOW_SECONDS),
        )
        .await;
    assert_eq!(refused.0, 401);
    assert!(refused.1.is_empty());
    assert!(
        owned_rows(&f).await == before,
        "unsigned body changes reserve or decide nothing"
    );
    assert_eq!(
        f.post_with(
            signed::CREATE,
            &body,
            &headers(&owner, &intent, NOW_SECONDS)
        )
        .await
        .1["result"],
        "applied"
    );
    f.close().await;
}

#[tokio::test]
async fn create_audience_array_requires_admin_and_applies_control() {
    let f = ready().await;
    let owner = f.owner();
    for (serial, audience, status) in [
        (0x724, json!(["https://other.example.invalid/admin"]), 401),
        (
            0x725,
            json!([
                "https://other.example.invalid/admin",
                signed::ADMIN_AUDIENCE
            ]),
            200,
        ),
    ] {
        let operation = operation(serial);
        let (body, intent) = create(&operation, "Audience", "");
        let before = owned_rows(&f).await;
        let headers = owner
            .headers_with(
                signed::CREATE,
                &intent,
                &signed::fresh_request_id(NOW_SECONDS).unwrap(),
                NOW_SECONDS,
                &|claims| {
                    claims["aud"] = audience.clone();
                },
            )
            .unwrap();
        let answer = f.post_bytes(signed::CREATE, &body, &headers).await;
        assert_eq!(answer.0, status);
        if status == 401 {
            assert!(answer.1.is_empty());
            assert!(owned_rows(&f).await == before);
        } else {
            let value: Value = serde_json::from_slice(&answer.1).unwrap();
            assert_eq!(value["result"], "applied");
        }
    }
    f.close().await;
}

#[tokio::test]
async fn create_requires_fresh_factor_bound_proof_and_dpop_target() {
    let f = ready().await;
    for case in [
        "no factor",
        "old factor",
        "bearer",
        "wrong target",
        "wrong proof key",
    ] {
        let operation = operation(0x726);
        let (body, intent) = create(&operation, "Bound", "");
        let before = owned_rows(&f).await;
        let mut owner = f.owner();
        if case == "no factor" {
            owner.second_factor_at = None;
        } else if case == "old factor" {
            owner.second_factor_at = Some(NOW_SECONDS - 901);
        }
        let mut proof_headers = headers(&owner, &intent, NOW_SECONDS);
        if case == "bearer"
            && let Some((_, authorization)) = proof_headers
                .iter_mut()
                .find(|(name, _)| *name == "authorization")
        {
            *authorization = authorization.replacen("DPoP ", "Bearer ", 1);
        }
        if case == "wrong target" {
            proof_headers = owner
                .headers(
                    LOOKUP,
                    &intent,
                    &signed::fresh_request_id(NOW_SECONDS).unwrap(),
                    NOW_SECONDS,
                )
                .unwrap();
        }
        if case == "wrong proof key" {
            let other = f.owner();
            let token = owner.token(NOW_SECONDS, &|_| {}).unwrap();
            let proof=other.key.proof(&json!({"jti":signed::fresh_request_id(NOW_SECONDS).unwrap(),"htm":"POST","htu":format!("{}{}",signed::PUBLIC_ORIGIN,signed::CREATE),"iat":NOW_SECONDS-1,"ath":signed::sha256_url(token.as_bytes()),"intent":signed::sha256_url(signed::canonical(&intent).as_bytes())})).unwrap();
            proof_headers = vec![("authorization", format!("DPoP {token}")), ("dpop", proof)];
        }
        let refused = f.post_bytes(signed::CREATE, &body, &proof_headers).await;
        if case == "no factor" || case == "old factor" {
            assert_eq!(refused.0, 200);
            assert_eq!(
                serde_json::from_slice::<Value>(&refused.1).unwrap(),
                json!({"result":"refused","reason":"secondFactorRequired"})
            );
        } else {
            assert_eq!(refused.0, 401);
            assert!(refused.1.is_empty());
        }
        assert!(
            owned_rows(&f).await == before,
            "{case}: refusal must write nothing"
        );
    }
    assert_eq!(
        submit(&f, &f.owner(), &operation(0x726), "Bound").await.1["result"],
        "applied"
    );
    f.close().await;
}
