use super::support::*;

#[expect(
    clippy::panic_in_result_fn,
    reason = "A genuine final approval allowance-record failure rolls back machine, approval, allowance and audit writes."
)]
#[tokio::test]
async fn approval_late_record_failure_rolls_back_all_rows_then_same_request_applies_once()
-> Result<()> {
    let f = MachineFixture::new().await?;
    f.execute("CREATE TRIGGER fail_login_approval BEFORE INSERT ON signin_change_records WHEN NEW.kind='allowanceChanged' AND NEW.target_id='019f0000-0000-7000-8000-00000000bc04' BEGIN SELECT RAISE(ABORT,'fixture last allowance record failure'); END",vec![]).await?;
    let body = approval(
        0xc901,
        INSTALLATION,
        "Atomic approval",
        &[WORKSPACE, WORKSPACE_TWO],
        f.now(),
    );
    let before = snapshot(&f).await?;
    assert_eq!(f.post(DEVICE, &body).await?, (503, Value::Null));
    assert!(
        snapshot(&f).await? == before,
        "last allowance record failure left partial machine, approval or audit"
    );
    f.execute("DROP TRIGGER fail_login_approval", vec![])
        .await?;
    let approved = f.post(DEVICE, &body).await?;
    assert_eq!(approved.0, 200);
    enrollment(&approved.1)?;
    let committed = snapshot(&f).await?;
    assert_eq!(f.post(DEVICE, &body).await?, approved);
    assert!(snapshot(&f).await? == committed);
    assert_eq!(f.rows("SELECT (SELECT COUNT(*) FROM login_machines),(SELECT COUNT(*) FROM login_machine_approvals),(SELECT COUNT(*) FROM signin_change_records WHERE kind='allowanceChanged')").await?,vec![vec![number(1)?,number(1)?,number(2)?]]);
    f.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The final MachineEnrolled record failure must restore the previous active login and all generation and audit facts."
)]
#[tokio::test]
async fn superseding_grant_last_record_failure_restores_old_login_generation_and_records_then_retries_once()
-> Result<()> {
    let f = MachineFixture::new().await?;
    let a = enrollment(
        &f.post(
            DEVICE,
            &approval(0xca01, INSTALLATION, "A", &[WORKSPACE], f.now()),
        )
        .await?
        .1,
    )?;
    assert_eq!(f.post(TOKEN, &token(&a, &a)).await?.0, 200);
    let b = enrollment(
        &f.post(
            DEVICE,
            &approval(0xca02, INSTALLATION, "B", &[WORKSPACE_TWO], f.now()),
        )
        .await?
        .1,
    )?;
    f.execute("CREATE TRIGGER fail_login_activation BEFORE INSERT ON signin_change_records WHEN NEW.kind='machineEnrolled' BEGIN SELECT RAISE(ABORT,'fixture last activation record failure'); END",vec![]).await?;
    let before = snapshot(&f).await?;
    assert_eq!(f.post(TOKEN, &token(&b, &b)).await?, (503, Value::Null));
    assert!(
        snapshot(&f).await? == before,
        "failed final record partially ended old login, activated new login, bumped generation or inserted old revoke record"
    );
    f.execute("DROP TRIGGER fail_login_activation", vec![])
        .await?;
    assert_eq!(
        f.post(TOKEN, &token(&a, &a)).await?,
        (200, grant(&a, INSTALLATION, f.now(), None)),
        "previous login remains usable after rollback"
    );
    assert_eq!(
        f.post(TOKEN, &token(&b, &b)).await?,
        (200, grant(&b, INSTALLATION, f.now(), Some((&a, &a))))
    );
    assert_eq!(
        f.rows("SELECT enrollment_generation FROM login_machines")
            .await?,
        vec![vec![number(2)?]]
    );
    assert_eq!(f.rows("SELECT kind,COUNT(*) FROM signin_change_records WHERE kind IN ('machineRevoked','machineEnrolled') GROUP BY kind ORDER BY kind").await?,vec![vec![text("machineEnrolled"),number(2)?],vec![text("machineRevoked"),number(1)?]]);
    let records = f
        .rows("SELECT * FROM signin_change_records ORDER BY id")
        .await?;
    assert_eq!(
        f.post(TOKEN, &token(&b, &b)).await?,
        (200, grant(&b, INSTALLATION, f.now(), Some((&a, &a))))
    );
    assert_eq!(
        f.rows("SELECT * FROM signin_change_records ORDER BY id")
            .await?,
        records
    );
    f.close().await
}
