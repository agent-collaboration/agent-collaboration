use super::support::*;

#[expect(
    clippy::panic_in_result_fn,
    reason = "N10 checks the real code deadline one millisecond before, at equality and one millisecond after without disturbing the older active login."
)]
#[tokio::test]
async fn first_grant_uses_the_codes_exact_deadline_and_lapse_keeps_the_working_login_and_allowances()
-> Result<()> {
    for elapsed_ms in [29_999, 30_000, 30_001] {
        let f = MachineFixture::new().await?;
        let a = enrollment(
            &f.post(
                DEVICE,
                &approval(0xc601, INSTALLATION, "Working", &[WORKSPACE], f.now()),
            )
            .await?
            .1,
        )?;
        assert_eq!(f.post(TOKEN, &token(&a, &a)).await?.0, 200);
        let deadline = f.now() + 30;
        let mut pending = approval(0xc602, INSTALLATION, "Pending", &[WORKSPACE_TWO], f.now());
        pending["codeExpiresAt"] = json!(deadline);
        let b = enrollment(&f.post(DEVICE, &pending).await?.1)?;
        let machine = f
            .rows("SELECT * FROM login_machines ORDER BY machine_id")
            .await?;
        let immutable_pending = f.rows("SELECT approval_id,request_id,machine_id,label,allowed_workspaces,approved_by_person_id,permission_checked,base_generation,code_expires_at,created_at FROM login_machine_approvals WHERE label='Pending'").await?;
        let working = f
            .rows("SELECT * FROM login_machine_approvals WHERE label='Working'")
            .await?;
        let records = f
            .rows("SELECT * FROM signin_change_records ORDER BY id")
            .await?;
        let credentials = f
            .rows(
                "SELECT name FROM sqlite_schema WHERE type='table' AND name='machine_credentials'",
            )
            .await?;
        f.advance(elapsed_ms).await?;
        let answer = f.post(TOKEN, &token(&b, &b)).await?;
        if elapsed_ms >= 30_000 {
            assert_eq!(answer, (200, refused("enrollmentNotActive")));
            let ended_at = chrono::DateTime::from_timestamp_millis(
                deadline * 1000 + i64::try_from(elapsed_ms - 30_000)?,
            )
            .unwrap()
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
            assert_eq!(
                f.rows("SELECT state,end_reason,machine_client_id,last_grant_at,ended_at,updated_at FROM login_machine_approvals WHERE label='Pending'").await?,
                vec![vec![
                    text("ended"),
                    text("lapsed"),
                    central_storage::SqlValue::Null,
                    central_storage::SqlValue::Null,
                    text(&ended_at),
                    text(&ended_at),
                ]]
            );
            assert!(
                f.rows("SELECT * FROM login_machines ORDER BY machine_id")
                    .await?
                    == machine,
                "lapse leaves machine generation/current allowances alone"
            );
            assert!(f.rows("SELECT approval_id,request_id,machine_id,label,allowed_workspaces,approved_by_person_id,permission_checked,base_generation,code_expires_at,created_at FROM login_machine_approvals WHERE label='Pending'").await? == immutable_pending);
            assert!(
                f.rows("SELECT * FROM login_machine_approvals WHERE label='Working'")
                    .await?
                    == working
            );
            assert!(
                f.rows("SELECT * FROM signin_change_records ORDER BY id")
                    .await?
                    == records
            );
            assert!(
                f.rows("SELECT name FROM sqlite_schema WHERE type='table' AND name='machine_credentials'")
                    .await?
                    == credentials
            );
            assert_eq!(
                f.post(TOKEN, &token(&a, &a)).await?,
                (200, grant(&a, INSTALLATION, f.now(), None)),
                "old login remains active"
            );
        } else {
            assert_eq!(
                answer,
                (200, grant(&b, INSTALLATION, deadline - 1, Some((&a, &a))))
            );
        }
        f.close().await?;
    }
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Renewal and exact 30-day idle expiry must use the one injected clock and write automatic multi-record effects."
)]
#[tokio::test]
async fn renewal_extends_idle_deadline_and_thirty_days_without_grant_expires_once() -> Result<()> {
    let f = MachineFixture::new().await?;
    let enrollment_id = enrollment(
        &f.post(
            DEVICE,
            &approval(
                0xc701,
                INSTALLATION,
                "Renewing",
                &[WORKSPACE, WORKSPACE_TWO],
                f.now(),
            ),
        )
        .await?
        .1,
    )?;
    assert_eq!(
        f.post(TOKEN, &token(&enrollment_id, &enrollment_id))
            .await?,
        (200, grant(&enrollment_id, INSTALLATION, f.now(), None))
    );
    // Renew after several complete token lifetimes. Active renewal does not use
    // the expired approval code or require another person/second factor.
    f.advance(1_800_000).await?;
    assert_eq!(
        f.post(TOKEN, &token(&enrollment_id, &enrollment_id))
            .await?,
        (200, grant(&enrollment_id, INSTALLATION, f.now(), None))
    );
    let renewed = f.now();
    assert_eq!(
        f.rows("SELECT enrollment_generation FROM login_machines")
            .await?,
        vec![vec![number(1)?]]
    );
    f.advance(30 * 24 * 60 * 60 * 1000 - 1000).await?;
    assert_eq!(
        f.post(TOKEN, &token(&enrollment_id, &enrollment_id))
            .await?,
        (200, grant(&enrollment_id, INSTALLATION, f.now(), None)),
        "a grant one second before idle expiry restarts the idle clock"
    );
    let last = f.now();
    assert!(last > renewed);
    f.advance(30 * 24 * 60 * 60 * 1000).await?;
    assert_eq!(
        f.post(TOKEN, &token(&enrollment_id, &enrollment_id))
            .await?,
        (200, refused("idleExpired"))
    );
    assert_eq!(
        f.rows("SELECT state,enrollment_generation,allowed_workspaces FROM login_machines")
            .await?,
        vec![vec![text("revoked"), number(2)?, text("[]")]]
    );
    let records=f.rows("SELECT request_id,kind,origin,actor_person_id,permission_checked,target_id FROM signin_change_records WHERE origin='automatic' ORDER BY id").await?;
    assert_eq!(
        records,
        vec![
            vec![
                text(format!("{enrollment_id}:machineExpired:{enrollment_id}")),
                text("machineExpired"),
                text("automatic"),
                central_storage::SqlValue::Null,
                central_storage::SqlValue::Null,
                text(&enrollment_id)
            ],
            vec![
                text(format!("{enrollment_id}:allowanceChanged:{WORKSPACE}")),
                text("allowanceChanged"),
                text("automatic"),
                central_storage::SqlValue::Null,
                central_storage::SqlValue::Null,
                text(WORKSPACE)
            ],
            vec![
                text(format!("{enrollment_id}:allowanceChanged:{WORKSPACE_TWO}")),
                text("allowanceChanged"),
                text("automatic"),
                central_storage::SqlValue::Null,
                central_storage::SqlValue::Null,
                text(WORKSPACE_TWO)
            ]
        ]
    );
    let before = snapshot(&f).await?;
    assert_eq!(
        f.post(TOKEN, &token(&enrollment_id, &enrollment_id))
            .await?,
        (200, refused("enrollmentNotActive"))
    );
    assert!(
        snapshot(&f).await? == before,
        "idle expiry cannot apply its withdrawal or automatic records twice"
    );
    f.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Unknown IDs, wrong clients, another physical org and neighboring installations cannot change each other's enrollment state."
)]
#[tokio::test]
async fn clients_organizations_unknown_ids_and_two_machine_logins_remain_isolated() -> Result<()> {
    let f = MachineFixture::new().await?;
    let a = enrollment(
        &f.post(
            DEVICE,
            &approval(0xc801, INSTALLATION, "A", &[WORKSPACE], f.now()),
        )
        .await?
        .1,
    )?;
    let b = enrollment(
        &f.post(
            DEVICE,
            &approval(0xc802, INSTALLATION_TWO, "B", &[WORKSPACE_TWO], f.now()),
        )
        .await?
        .1,
    )?;
    for body in [token(&id(0xc899), &id(0xc899)), token(&a, &b)] {
        let before = snapshot(&f).await?;
        let expected = if body.get("enrollmentId") == Some(&json!(a)) {
            "wrongClient"
        } else {
            "enrollmentNotActive"
        };
        assert_eq!(f.post(TOKEN, &body).await?, (200, refused(expected)));
        assert!(snapshot(&f).await? == before);
    }
    assert_eq!(
        f.post(TOKEN, &token(&a, &a)).await?,
        (200, grant(&a, INSTALLATION, f.now(), None))
    );
    assert_eq!(
        f.post(TOKEN, &token(&b, &b)).await?,
        (200, grant(&b, INSTALLATION_TWO, f.now(), None))
    );
    let before = snapshot(&f).await?;
    assert_eq!(
        f.post(TOKEN, &token(&a, &b)).await?,
        (200, refused("wrongClient"))
    );
    assert!(snapshot(&f).await? == before);
    f.other_org().await?;
    let mut foreign = token(&a, &a);
    foreign["organizationId"] = json!(OTHER_ORG);
    assert_eq!(
        f.post(TOKEN, &foreign).await?,
        (200, refused("enrollmentNotActive"))
    );
    assert!(
        snapshot(&f).await? == before,
        "foreign org lookup rewrote the original organization"
    );
    assert_eq!(
        f.rows("SELECT enrollment_generation FROM login_machines ORDER BY machine_id")
            .await?,
        vec![vec![number(1)?], vec![number(1)?]]
    );
    assert_eq!(
        f.rows("SELECT COUNT(*) FROM login_machine_approvals WHERE state='enrolled'")
            .await?,
        vec![vec![number(2)?]]
    );
    f.close().await
}
