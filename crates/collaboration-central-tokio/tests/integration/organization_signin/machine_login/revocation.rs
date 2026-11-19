//! BA1 owner revocation uses only the approved closed real-store test control.
//! Connection/session close delivery and the public revoke route belong to C.
use super::support::*;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Real generation transitions A-approved/B-active/B-revoked must permanently fence late A and revoked B token grants."
)]
#[tokio::test]
async fn revocation_repeat_and_aba_never_reactivate_old_approvals_and_new_approval_uses_new_generation()
-> Result<()> {
    let mut f = MachineFixture::new().await?;
    let approval_a = approval(0xce01, INSTALLATION, "A", &[WORKSPACE], f.now());
    let a = enrollment(&f.post(DEVICE, &approval_a).await?.1)?;
    let b = enrollment(
        &f.post(
            DEVICE,
            &approval(0xce02, INSTALLATION, "B", &[WORKSPACE_TWO], f.now()),
        )
        .await?
        .1,
    )?;
    assert_eq!(
        f.post(TOKEN, &token(&b, &b)).await?,
        (200, grant(&b, INSTALLATION, f.now(), None))
    );
    assert_eq!(
        f.rows("SELECT enrollment_generation FROM machines").await?,
        vec![vec![number(1)?]]
    );
    // An unrelated machine_id/enrollment pair cannot revoke B's active login.
    let other = enrollment(
        &f.post(
            DEVICE,
            &approval(0xce03, INSTALLATION_TWO, "Neighbor", &[WORKSPACE], f.now()),
        )
        .await?
        .1,
    )?;
    assert_eq!(f.post(TOKEN, &token(&other, &other)).await?.0, 200);
    let neighbor = f
        .rows("SELECT * FROM machines WHERE machine_id='019f0000-0000-7000-8000-00000000bc06'")
        .await?;
    let before = snapshot(&f).await?;
    assert!(
        f.revoke(0xce10, INSTALLATION, &other).await??.is_none(),
        "mismatched current enrollment cannot revoke another machine_id"
    );
    assert!(snapshot(&f).await? == before);
    // Historical BA1's second pending approval is made after B's first Grant.
    // Its base must equal the current generation immediately before revocation;
    // only the revoke bump, not B's earlier activation, can fence this approval.
    let pending_before_revoke = enrollment(
        &f.post(
            DEVICE,
            &approval(
                0xce05,
                INSTALLATION,
                "Pending before revoke",
                &[WORKSPACE],
                f.now(),
            ),
        )
        .await?
        .1,
    )?;
    assert_eq!(
        f.rows("SELECT a.base_generation,m.enrollment_generation,a.state FROM machine_approvals a JOIN machines m ON m.machine_id=a.machine_id WHERE a.label='Pending before revoke'").await?,
        vec![vec![number(1)?, number(1)?, text("approved")]]
    );
    let ended = f
        .revoke(0xce11, INSTALLATION, &b)
        .await??
        .ok_or("active owner revocation did not return its ended client")?;
    assert_eq!(
        serde_json::to_value(ended)?,
        json!({"enrollmentId":b,"clientId":b})
    );
    assert_eq!(f.rows("SELECT state,enrollment_generation,allowed_workspaces FROM machines WHERE machine_id='019f0000-0000-7000-8000-00000000bc05'").await?,vec![vec![text("revoked"),number(2)?,text("[]")]]);
    assert_eq!(
        f.rows("SELECT * FROM machines WHERE machine_id='019f0000-0000-7000-8000-00000000bc06'")
            .await?,
        neighbor,
        "owner revocation is machine_id-scoped"
    );
    let revoked = snapshot(&f).await?;
    assert_eq!(
        f.post(
            TOKEN,
            &token(&pending_before_revoke, &pending_before_revoke)
        )
        .await?,
        (200, refused("superseded")),
        "an approval made at generation 1 is fenced by revocation's own bump to 2"
    );
    assert!(snapshot(&f).await? == revoked);
    assert_eq!(
        f.post(TOKEN, &token(&a, &a)).await?,
        (200, refused("superseded")),
        "A approved at generation0 cannot activate after B activation+revocation generation2"
    );
    assert_eq!(
        f.post(TOKEN, &token(&b, &b)).await?,
        (200, refused("enrollmentNotActive")),
        "repeating revoked B never mints a Grant"
    );
    assert!(
        f.revoke(0xce12, INSTALLATION, &b).await??.is_none(),
        "repeat revoke reports no current login"
    );
    assert!(
        snapshot(&f).await? == revoked,
        "refused token/repeat revoke changed rows or emitted duplicate records"
    );
    f.reopen().await?;
    assert_eq!(
        f.post(
            TOKEN,
            &token(&pending_before_revoke, &pending_before_revoke)
        )
        .await?,
        (200, refused("superseded")),
        "the generation 1 pending approval remains fenced after owning reopen"
    );
    assert!(snapshot(&f).await? == revoked);
    assert_eq!(
        f.post(TOKEN, &token(&a, &a)).await?,
        (200, refused("superseded")),
        "ABA fence survives owning reopen"
    );
    assert_eq!(
        f.post(TOKEN, &token(&b, &b)).await?,
        (200, refused("enrollmentNotActive"))
    );
    assert!(snapshot(&f).await? == revoked);
    let replay = f.post(DEVICE, &approval_a).await?;
    assert_eq!(
        replay,
        (
            200,
            json!({"result":"approved","enrollmentId":a,"label":"A","allowedWorkspaces":[WORKSPACE]})
        ),
        "N03 remains the immutable first approval even after owner revocation"
    );
    assert!(snapshot(&f).await? == revoked);
    let c = enrollment(
        &f.post(
            DEVICE,
            &approval(0xce04, INSTALLATION, "C", &[WORKSPACE], f.now()),
        )
        .await?
        .1,
    )?;
    assert_eq!(f.rows("SELECT state,enrollment_generation FROM machines WHERE machine_id='019f0000-0000-7000-8000-00000000bc05'").await?,vec![vec![text("revoked"),number(2)?]],"new approval alone does not make a revoked machine enrolled");
    assert_eq!(
        f.rows("SELECT base_generation FROM machine_approvals WHERE label='C'")
            .await?,
        vec![vec![number(2)?]]
    );
    assert_eq!(
        f.post(TOKEN, &token(&c, &c)).await?,
        (200, grant(&c, INSTALLATION, f.now(), None))
    );
    assert_eq!(f.rows("SELECT state,enrollment_generation FROM machines WHERE machine_id='019f0000-0000-7000-8000-00000000bc05'").await?,vec![vec![text("enrolled"),number(3)?]]);
    assert_eq!(
        f.post(TOKEN, &token(&other, &other)).await?,
        (200, grant(&other, INSTALLATION_TWO, f.now(), None))
    );
    let records=f.rows("SELECT request_id,kind,origin,actor_person_id,permission_checked,target_id FROM organization_change_records WHERE permission_checked='machine:revoke' ORDER BY id").await?;
    assert_eq!(
        records,
        vec![
            vec![
                text(format!("{}:machineRevoked:{b}", id(0xce11))),
                text("machineRevoked"),
                text("person"),
                text(signed::OWNER),
                text("machine:revoke"),
                text(&b)
            ],
            vec![
                text(format!("{}:allowanceChanged:{WORKSPACE}", id(0xce11))),
                text("allowanceChanged"),
                text("person"),
                text(signed::OWNER),
                text("machine:revoke"),
                text(WORKSPACE)
            ],
            vec![
                text(format!("{}:allowanceChanged:{WORKSPACE_TWO}", id(0xce11))),
                text("allowanceChanged"),
                text("person"),
                text(signed::OWNER),
                text("machine:revoke"),
                text(WORKSPACE_TWO)
            ]
        ]
    );
    assert_eq!(
        f.rows("SELECT COUNT(*) FROM machine_approvals WHERE state='enrolled'")
            .await?,
        vec![vec![number(2)?]]
    );
    f.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The final owner-withdrawal record failure must roll back revocation, allowance deletion, generation and prior records together."
)]
#[tokio::test]
async fn owner_revocation_final_record_failure_keeps_active_client_generation_and_allowances_until_real_retry()
-> Result<()> {
    let f = MachineFixture::new().await?;
    let a = enrollment(
        &f.post(
            DEVICE,
            &approval(
                0xcf01,
                INSTALLATION,
                "Active",
                &[WORKSPACE, WORKSPACE_TWO],
                f.now(),
            ),
        )
        .await?
        .1,
    )?;
    assert_eq!(
        f.post(TOKEN, &token(&a, &a)).await?,
        (200, grant(&a, INSTALLATION, f.now(), None))
    );
    f.execute("CREATE TRIGGER fail_owner_revoke BEFORE INSERT ON organization_change_records WHEN NEW.kind='allowanceChanged' AND NEW.permission_checked='machine:revoke' AND NEW.target_id='019f0000-0000-7000-8000-00000000bc04' BEGIN SELECT RAISE(ABORT,'fixture final owner withdrawal record failure'); END",vec![]).await?;
    let before = snapshot(&f).await?;
    let failed = f.revoke(0xcf02, INSTALLATION, &a).await?;
    assert!(
        failed.is_err(),
        "real last withdrawal record failure was swallowed"
    );
    assert!(
        snapshot(&f).await? == before,
        "failed owner revoke ended client, cleared allowances, bumped generation or retained a partial record"
    );
    f.execute("DROP TRIGGER fail_owner_revoke", vec![]).await?;
    assert_eq!(
        f.post(TOKEN, &token(&a, &a)).await?,
        (200, grant(&a, INSTALLATION, f.now(), None)),
        "the current login is still usable after failed revocation"
    );
    let ended = f
        .revoke(0xcf02, INSTALLATION, &a)
        .await??
        .ok_or("retry did not revoke the active client")?;
    assert_eq!(
        serde_json::to_value(ended)?,
        json!({"enrollmentId":a,"clientId":a})
    );
    assert_eq!(
        f.rows("SELECT state,enrollment_generation,allowed_workspaces FROM machines")
            .await?,
        vec![vec![text("revoked"), number(2)?, text("[]")]]
    );
    assert_eq!(f.rows("SELECT kind,COUNT(*) FROM organization_change_records WHERE permission_checked='machine:revoke' GROUP BY kind ORDER BY kind").await?,vec![vec![text("allowanceChanged"),number(2)?],vec![text("machineRevoked"),number(1)?]]);
    let committed = snapshot(&f).await?;
    assert_eq!(
        f.post(TOKEN, &token(&a, &a)).await?,
        (200, refused("enrollmentNotActive"))
    );
    assert!(f.revoke(0xcf02, INSTALLATION, &a).await??.is_none());
    assert!(
        snapshot(&f).await? == committed,
        "repeat revoke/token refusal emitted another record or generation"
    );
    f.close().await
}
