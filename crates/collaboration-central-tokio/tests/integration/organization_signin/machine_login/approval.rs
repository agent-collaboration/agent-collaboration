use super::support::*;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Exact private reply, immutable approval and owning-row assertions propagate real fixture errors."
)]
#[tokio::test]
async fn approval_replays_immutable_three_field_snapshot_after_changed_input_activation_and_reopen()
-> Result<()> {
    let mut f = MachineFixture::new().await?;
    let request = approval(
        0xc101,
        INSTALLATION,
        "Original label",
        &[WORKSPACE, WORKSPACE_TWO],
        f.now(),
    );
    let first = f.post(DEVICE, &request).await?;
    assert_eq!(first.0, 200);
    let enrollment_id = enrollment(&first.1)?;
    assert_eq!(
        first.1,
        json!({"result":"approved","enrollmentId":enrollment_id,"label":"Original label","allowedWorkspaces":[WORKSPACE,WORKSPACE_TWO]})
    );
    assert_eq!(
        f.rows("SELECT state,enrollment_generation FROM login_machines")
            .await?,
        vec![vec![text("revoked"), number(0)?]],
        "approval admits nothing"
    );
    assert_eq!(
        f.rows(
            "SELECT state,base_generation,machine_client_id,last_grant_at FROM login_machine_approvals"
        )
        .await?,
        vec![vec![
            text("approved"),
            number(0)?,
            central_storage::SqlValue::Null,
            central_storage::SqlValue::Null
        ]]
    );
    assert!(
        f.rows("SELECT name FROM sqlite_schema WHERE type='table' AND name='machine_credentials'")
            .await?
            .is_empty()
    );
    let before = snapshot(&f).await?;
    let mut changed = request.clone();
    changed["label"] = json!("Submitted changed label");
    changed["allowedWorkspaces"] = json!([id(0xc199)]);
    changed["approverSecondFactorAt"] = Value::Null;
    assert_eq!(
        f.post(DEVICE, &changed).await?,
        first,
        "N03 returns the first approval even when mutable submitted fields change"
    );
    assert!(
        snapshot(&f).await? == before,
        "repeat approval wrote a second row or record"
    );
    assert_eq!(
        f.post(TOKEN, &token(&enrollment_id, &enrollment_id))
            .await?,
        (200, grant(&enrollment_id, INSTALLATION, f.now(), None))
    );
    let activated = snapshot(&f).await?;
    assert_eq!(
        f.post(DEVICE, &changed).await?,
        first,
        "activation cannot replace approval's immutable reply"
    );
    assert!(snapshot(&f).await? == activated);
    f.reopen().await?;
    f.advance(86_400_001).await?;
    assert_eq!(
        f.post(DEVICE, &changed).await?,
        first,
        "owning reopen and a day beyond code expiry retain the exact three-field approval"
    );
    assert!(snapshot(&f).await? == activated);
    let rows=f.rows("SELECT request_id,kind,origin,actor_person_id,permission_checked,target_id FROM signin_change_records WHERE kind='allowanceChanged' ORDER BY target_id").await?;
    assert_eq!(
        rows,
        vec![
            vec![
                text(format!("{}:allowanceChanged:{WORKSPACE}", id(0xc101))),
                text("allowanceChanged"),
                text("person"),
                text(signed::OWNER),
                text("machine:enroll"),
                text(WORKSPACE)
            ],
            vec![
                text(format!("{}:allowanceChanged:{WORKSPACE_TWO}", id(0xc101))),
                text("allowanceChanged"),
                text("person"),
                text(signed::OWNER),
                text("machine:enroll"),
                text(WORKSPACE_TWO)
            ]
        ]
    );
    f.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "A retained request belongs to its original machine_id, while same-machine_id N03 replay retains the first snapshot after expiry and reopen."
)]
#[tokio::test]
async fn retained_approval_refuses_another_installation_but_preserves_same_installation_replay_after_expiry()
-> Result<()> {
    let mut f = MachineFixture::new().await?;
    let original = approval(
        0xd501,
        INSTALLATION,
        "Original machine_id",
        &[WORKSPACE],
        f.now(),
    );
    let first = f.post(DEVICE, &original).await?;
    assert_eq!(first.0, 200);
    let enrollment_id = enrollment(&first.1)?;
    assert_eq!(
        first.1,
        json!({"result":"approved","enrollmentId":enrollment_id,"label":"Original machine_id","allowedWorkspaces":[WORKSPACE]})
    );
    assert_eq!(
        f.post(TOKEN, &token(&enrollment_id, &enrollment_id))
            .await?,
        (200, grant(&enrollment_id, INSTALLATION, f.now(), None)),
        "the original approval is a working positive control"
    );

    let mut another_installation = original.clone();
    another_installation["installation"] = json!(INSTALLATION_TWO);

    // The new machine_id/peer pair is internally consistent. Only ownership
    // of the already-retained request differs.
    let before = snapshot(&f).await?;
    assert_eq!(
        f.post(DEVICE, &another_installation).await?,
        (
            200,
            json!({"result":"refused","reason":{"kind":"notPermitted"}})
        )
    );
    assert!(
        snapshot(&f).await? == before,
        "foreign-machine_id replay changed owning rows"
    );

    f.reopen().await?;
    f.advance(600_001).await?;
    let mut same_installation = original.clone();
    same_installation["label"] = json!("Changed submitted label");
    same_installation["allowedWorkspaces"] = json!([id(0xd599)]);
    same_installation["approverSecondFactorAt"] = Value::Null;
    same_installation["codeExpiresAt"] = json!(f.now() - 1);
    assert_eq!(
        f.post(DEVICE, &same_installation).await?,
        first,
        "same-machine_id N03 replay precedes new-approval deadline and mutable-input checks"
    );
    assert!(snapshot(&f).await? == before);
    another_installation["codeExpiresAt"] = json!(f.now() + 600);
    assert_eq!(
        f.post(DEVICE, &another_installation).await?,
        (
            200,
            json!({"result":"refused","reason":{"kind":"notPermitted"}})
        ),
        "retained machine_id ownership survives owning reopen and original code expiry"
    );
    assert!(snapshot(&f).await? == before);
    f.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "New expired-code approvals must refuse at the applying sample without reserving facts; the same request with a future deadline must still approve and Grant."
)]
#[tokio::test]
async fn new_approval_refuses_codes_at_or_before_applying_now_without_effects_and_future_code_grants()
-> Result<()> {
    // The hold cases start with a future code at receipt, then cross its exact
    // deadline before the decision samples the one injected clock.
    for (deadline_offset_seconds, hold_millis) in [(-1, 0), (0, 0), (1, 1_000), (1, 1_001)] {
        let f = MachineFixture::new().await?;
        let mut request = approval(
            0xd601,
            INSTALLATION,
            "Deadline approval",
            &[WORKSPACE, WORKSPACE_TWO],
            f.now(),
        );
        request["codeExpiresAt"] = json!(f.now() + deadline_offset_seconds);
        let before = snapshot(&f).await?;
        let answer = if hold_millis == 0 {
            f.post(DEVICE, &request).await?
        } else {
            f.held_approval(&request, hold_millis).await?
        };
        assert_eq!(
            answer,
            (
                200,
                json!({"result":"refused","reason":{"kind":"notPermitted"}})
            ),
            "new approval deadline offset {deadline_offset_seconds}s and held {hold_millis}ms"
        );
        assert!(
            snapshot(&f).await? == before,
            "expired code reserved an approval, changed a machine or emitted allowance records"
        );

        // Reuse the same request identity: refusal must not reserve it. One
        // second in the future is enough; no guessed upper bound is introduced.
        request["codeExpiresAt"] = json!(f.now() + 1);
        let approved = f.post(DEVICE, &request).await?;
        assert_eq!(approved.0, 200);
        let enrollment_id = enrollment(&approved.1)?;
        assert_eq!(
            approved.1,
            json!({"result":"approved","enrollmentId":enrollment_id,"label":"Deadline approval","allowedWorkspaces":[WORKSPACE,WORKSPACE_TWO]})
        );
        assert_eq!(
            f.post(TOKEN, &token(&enrollment_id, &enrollment_id))
                .await?,
            (200, grant(&enrollment_id, INSTALLATION, f.now(), None)),
            "unexpired approval remains usable after the no-effect refusal"
        );
        f.close().await?;
    }
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "An overflowing wire deadline must return a no-effect policy refusal and leave the same request usable for a valid approval and Grant."
)]
#[tokio::test]
async fn overflowing_new_approval_deadline_is_not_permitted_without_effects_and_future_retry_grants()
-> Result<()> {
    let f = MachineFixture::new().await?;
    let mut request = approval(
        0xd701,
        INSTALLATION,
        "Overflow deadline",
        &[WORKSPACE, WORKSPACE_TWO],
        f.now(),
    );
    // Exactly representable by JavaScript and i64, but seconds-to-ms overflows.
    // This must reach policy as a valid wire integer rather than round or fail 400.
    request["codeExpiresAt"] = json!(10_000_000_000_000_000_i64);
    let before = snapshot(&f).await?;
    assert_eq!(
        f.post(DEVICE, &request).await?,
        (
            200,
            json!({"result":"refused","reason":{"kind":"notPermitted"}})
        )
    );
    assert!(
        snapshot(&f).await? == before,
        "overflow refusal reserved approval, changed machine or emitted audit records"
    );

    // A refusal leaves request identity reusable with a representable future code.
    request["codeExpiresAt"] = json!(f.now() + 1);
    let approved = f.post(DEVICE, &request).await?;
    assert_eq!(approved.0, 200);
    let enrollment_id = enrollment(&approved.1)?;
    assert_eq!(
        approved.1,
        json!({"result":"approved","enrollmentId":enrollment_id,"label":"Overflow deadline","allowedWorkspaces":[WORKSPACE,WORKSPACE_TWO]})
    );
    assert_eq!(
        f.post(TOKEN, &token(&enrollment_id, &enrollment_id))
            .await?,
        (200, grant(&enrollment_id, INSTALLATION, f.now(), None))
    );
    f.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "First-machine exemption and subsequent factor boundaries are observed through actual approval requests."
)]
#[tokio::test]
async fn first_machine_factor_exemption_and_later_900_second_factor_boundary_are_exact()
-> Result<()> {
    let f = MachineFixture::new().await?;
    let mut first = approval(0xc201, INSTALLATION, "First", &[WORKSPACE], f.now());
    first["approverSecondFactorAt"] = Value::Null;
    let first = f.post(DEVICE, &first).await?;
    assert_eq!(first.0, 200);
    let id = enrollment(&first.1)?;
    assert_eq!(f.post(TOKEN, &token(&id, &id)).await?.0, 200);
    for factor in [None, Some(f.now() - 901), Some(f.now() + 1)] {
        let mut request = approval(0xc202, INSTALLATION_TWO, "Second", &[WORKSPACE], f.now());
        if let Some(at) = factor {
            request["approverSecondFactorAt"] = json!(at);
        } else {
            request["approverSecondFactorAt"] = Value::Null;
        }
        let before = snapshot(&f).await?;
        assert_eq!(
            f.post(DEVICE, &request).await?,
            (
                200,
                json!({"result":"refused","reason":{"kind":"secondFactorRequired","withinSeconds":900}})
            )
        );
        assert!(
            snapshot(&f).await? == before,
            "factor refusal changed machine/login state"
        );
    }
    let mut boundary = approval(0xc203, INSTALLATION_TWO, "Second", &[WORKSPACE], f.now());
    boundary["approverSecondFactorAt"] = json!(f.now() - 900);
    assert_eq!(
        f.post(DEVICE, &boundary).await?.1.get("result"),
        Some(&json!("approved"))
    );
    f.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Closed request/profile validation and unknown workspace refusals must leave all login effects unchanged."
)]
#[tokio::test]
async fn approval_uses_only_the_issuer_machine_id_and_refuses_invalid_requests() -> Result<()> {
    let f = MachineFixture::new().await?;
    let original = approval(0xc301, INSTALLATION, "Checked", &[WORKSPACE], f.now());
    for (case, expected_status) in [
        ("workspace", 200),
        ("permission", 400),
        ("former", 400),
        ("missing deadline", 400),
        ("missing factor", 400),
    ] {
        let mut request = original.clone();
        match case {
            "workspace" => request["allowedWorkspaces"] = json!([id(0xc399)]),
            "permission" => request["permissionChecked"] = json!("machine:revoke"),
            "former" => {
                request
                    .as_object_mut()
                    .unwrap()
                    .insert("formerServiceId".into(), json!(id(0xc398)));
            }
            "missing factor" => {
                request
                    .as_object_mut()
                    .unwrap()
                    .remove("approverSecondFactorAt");
            }
            _ => {
                request.as_object_mut().unwrap().remove("codeExpiresAt");
            }
        }
        let before = snapshot(&f).await?;
        let response = f.post(DEVICE, &request).await?;
        assert_eq!(response.0, expected_status, "{case}");
        if case == "workspace" {
            assert_eq!(
                response.1,
                json!({"result":"refused","reason":{"kind":"unknownWorkspace"}})
            );
        }
        assert!(snapshot(&f).await? == before, "{case} changed owning rows");
    }
    // The pinned issuer still sends a peer. Only the issued machine ID owns approval.
    let mut approval = original;
    approval["peer"] = json!(INSTALLATION_TWO);
    let (status, reply) = f.post(DEVICE, &approval).await?;
    assert_eq!(status, 200);
    assert_eq!(reply["result"], "approved");
    assert_eq!(
        f.rows("SELECT machine_id FROM login_machines").await?,
        vec![vec![text(INSTALLATION)]]
    );
    f.close().await
}
