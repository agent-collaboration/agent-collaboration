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
        f.rows("SELECT state,enrollment_generation FROM machines")
            .await?,
        vec![vec![text("revoked"), number(0)?]],
        "approval admits nothing"
    );
    assert_eq!(
        f.rows(
            "SELECT state,base_generation,machine_client_id,last_grant_at FROM machine_approvals"
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
        f.rows("SELECT * FROM machine_credentials")
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
    let rows=f.rows("SELECT request_id,kind,origin,actor_person_id,permission_checked,target_id FROM organization_change_records WHERE kind='allowanceChanged' ORDER BY target_id").await?;
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
async fn approval_rejects_peer_mismatch_unknown_workspace_and_wrong_closed_wire_fields()
-> Result<()> {
    let f = MachineFixture::new().await?;
    let original = approval(0xc301, INSTALLATION, "Checked", &[WORKSPACE], f.now());
    for (case, expected_status) in [
        ("peer", 200),
        ("workspace", 200),
        ("permission", 400),
        ("former", 400),
        ("missing deadline", 400),
        ("missing factor", 400),
    ] {
        let mut request = original.clone();
        match case {
            "peer" => request["peer"] = json!(INSTALLATION_TWO),
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
        if case == "peer" {
            assert_eq!(
                response.1,
                json!({"result":"refused","reason":{"kind":"notPermitted"}})
            );
        }
        if case == "workspace" {
            assert_eq!(
                response.1,
                json!({"result":"refused","reason":{"kind":"unknownWorkspace"}})
            );
        }
        assert!(snapshot(&f).await? == before, "{case} changed owning rows");
    }
    f.close().await
}
