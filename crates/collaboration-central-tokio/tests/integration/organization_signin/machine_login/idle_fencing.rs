use super::support::*;

#[expect(
    clippy::panic_in_result_fn,
    reason = "BA1 fences approvals made before automatic idle revocation using the actual decision routes and owning rows."
)]
#[tokio::test]
async fn idle_expiry_fences_a_still_claimable_approval_and_fresh_approval_can_activate()
-> Result<()> {
    // Arrange: E1 is active at generation one. Approval-time allowance effects
    // remain intentional; only idle revocation must fence earlier approvals.
    let f = MachineFixture::new().await?;
    let first_reply = f
        .post(
            DEVICE,
            &approval(0xdb01, INSTALLATION, "Idle E1", &[], f.now()),
        )
        .await?;
    assert_eq!(first_reply.0, 200);
    let first = enrollment(&first_reply.1)?;
    assert_eq!(
        f.post(TOKEN, &token(&first, &first)).await?,
        (200, grant(&first, INSTALLATION, f.now(), None))
    );
    let first_granted_at = f.now();
    f.advance((30 * 24 * 60 * 60 - 60) * 1000).await?;
    let pending_body = approval(0xdb02, INSTALLATION, "Pending E2", &[WORKSPACE], f.now());
    let code_expires_at = pending_body["codeExpiresAt"].as_i64().unwrap();
    let pending_reply = f.post(DEVICE, &pending_body).await?;
    assert_eq!(pending_reply.0, 200);
    let pending = enrollment(&pending_reply.1)?;
    assert_eq!(
        pending_reply.1,
        json!({"result":"approved","enrollmentId":pending,"label":"Pending E2","allowedWorkspaces":[WORKSPACE]})
    );
    assert_eq!(
        f.rows("SELECT state,enrollment_generation,label,allowed_workspaces FROM login_machines")
            .await?,
        vec![vec![
            text("enrolled"),
            number(1)?,
            text("Pending E2"),
            text(serde_json::to_string(&vec![WORKSPACE])?)
        ]]
    );
    assert_eq!(
        f.rows("SELECT state,base_generation,machine_client_id FROM login_machine_approvals WHERE label='Pending E2'").await?,
        vec![vec![text("approved"), number(1)?, central_storage::SqlValue::Null]]
    );
    assert_eq!(
        f.rows("SELECT approval_id FROM login_machine_approvals WHERE state='enrolled'")
            .await?,
        vec![vec![text(&first)]]
    );
    assert_eq!(
        f.rows("SELECT request_id,kind,origin,actor_person_id,permission_checked,target_id FROM signin_change_records WHERE kind='allowanceChanged' ORDER BY id").await?,
        vec![vec![
            text(format!("{}:allowanceChanged:{WORKSPACE}", id(0xdb02))),
            text("allowanceChanged"), text("person"), text(signed::OWNER),
            text("machine:enroll"), text(WORKSPACE),
        ]]
    );

    // Act: E1's exact idle boundary revokes it while E2's real code remains live.
    f.advance(60_000).await?;
    assert_eq!(f.now(), first_granted_at + 30 * 24 * 60 * 60);
    assert!(f.now() < code_expires_at);
    f.execute("CREATE TRIGGER fail_idle_expiry BEFORE INSERT ON signin_change_records WHEN NEW.kind='allowanceChanged' AND NEW.origin='automatic' BEGIN SELECT RAISE(ABORT,'fixture final idle allowance record failure'); END",vec![]).await?;
    let before_failed_expiry = snapshot(&f).await?;
    assert_eq!(
        f.post(TOKEN, &token(&first, &first)).await?,
        (503, Value::Null)
    );
    assert!(
        snapshot(&f).await? == before_failed_expiry,
        "final idle record failure changed machine generation, allowances, approvals, records or credentials"
    );
    f.execute("DROP TRIGGER fail_idle_expiry", vec![]).await?;
    assert_eq!(
        f.post(TOKEN, &token(&first, &first)).await?,
        (200, refused("idleExpired"))
    );
    assert_eq!(
        f.rows("SELECT state,allowed_workspaces FROM login_machines")
            .await?,
        vec![vec![text("revoked"), text("[]")]]
    );
    assert_eq!(
        f.rows("SELECT state,end_reason FROM login_machine_approvals WHERE label='Idle E1'")
            .await?,
        vec![vec![text("ended"), text("idle")]]
    );
    let expired_snapshot = snapshot(&f).await?;
    let stale_claim = f.post(TOKEN, &token(&pending, &pending)).await?;

    // Assert: a live device code cannot cross the generation fence. On the old
    // implementation this actual request grants E2 with the cleared allowances.
    assert_eq!(stale_claim, (200, refused("superseded")));
    assert!(
        snapshot(&f).await? == expired_snapshot,
        "the stale first claim changed machine, approvals, records or credentials"
    );
    assert_eq!(
        f.rows("SELECT state,enrollment_generation,allowed_workspaces FROM login_machines")
            .await?,
        vec![vec![text("revoked"), number(2)?, text("[]")]]
    );
    assert_eq!(
        f.rows("SELECT COUNT(*) FROM login_machine_approvals WHERE state='enrolled'")
            .await?,
        vec![vec![number(0)?]]
    );
    assert_eq!(
        f.rows("SELECT state,base_generation,machine_client_id FROM login_machine_approvals WHERE label='Pending E2'").await?,
        vec![vec![text("approved"), number(1)?, central_storage::SqlValue::Null]]
    );
    assert_eq!(
        f.rows("SELECT request_id,kind,origin,target_id FROM signin_change_records WHERE origin='automatic' ORDER BY id").await?,
        vec![
            vec![text(format!("{first}:machineExpired:{first}")), text("machineExpired"), text("automatic"), text(&first)],
            vec![text(format!("{first}:allowanceChanged:{WORKSPACE}")), text("allowanceChanged"), text("automatic"), text(WORKSPACE)],
        ]
    );

    // A new approval observes generation two and may activate normally. This
    // positive control keeps approval effects and first-grant timing unchanged.
    let fresh_reply = f
        .post(
            DEVICE,
            &approval(0xdb03, INSTALLATION, "Fresh E3", &[WORKSPACE], f.now()),
        )
        .await?;
    assert_eq!(fresh_reply.0, 200);
    let fresh = enrollment(&fresh_reply.1)?;
    assert_eq!(
        f.rows("SELECT state,base_generation FROM login_machine_approvals WHERE label='Fresh E3'")
            .await?,
        vec![vec![text("approved"), number(2)?]]
    );
    assert_eq!(
        f.post(TOKEN, &token(&fresh, &fresh)).await?,
        (200, grant(&fresh, INSTALLATION, f.now(), None))
    );
    assert_eq!(
        f.rows("SELECT state,enrollment_generation,allowed_workspaces FROM login_machines")
            .await?,
        vec![vec![
            text("enrolled"),
            number(3)?,
            text(serde_json::to_string(&vec![WORKSPACE])?)
        ]]
    );
    assert_eq!(
        f.rows("SELECT approval_id FROM login_machine_approvals WHERE state='enrolled'")
            .await?,
        vec![vec![text(&fresh)]]
    );
    f.close().await
}
