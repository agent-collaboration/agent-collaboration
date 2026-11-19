use super::support::*;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Both BA1 grant orders and generation/one-active invariants run through real approval and token writes."
)]
#[tokio::test]
async fn two_same_generation_approvals_first_grant_wins_in_both_orders_and_active_repeat_never_bumps()
-> Result<()> {
    for reverse in [false, true] {
        let f = MachineFixture::new().await?;
        let a = f
            .post(
                DEVICE,
                &approval(0xc401, INSTALLATION, "A", &[WORKSPACE], f.now()),
            )
            .await?;
        let a = enrollment(&a.1);
        let b = f
            .post(
                DEVICE,
                &approval(0xc402, INSTALLATION, "B", &[WORKSPACE_TWO], f.now()),
            )
            .await?;
        let b = enrollment(&b.1);
        assert_eq!(
            f.rows("SELECT base_generation FROM login_machine_approvals ORDER BY approval_id")
                .await?,
            vec![vec![number(0)], vec![number(0)]]
        );
        let (winner, loser) = if reverse { (&b, &a) } else { (&a, &b) };
        assert_eq!(
            f.post(TOKEN, &token(winner, winner)).await?,
            (200, grant(winner, INSTALLATION, f.now(), None))
        );
        let before = snapshot(&f).await?;
        assert_eq!(
            f.post(TOKEN, &token(loser, loser)).await?,
            (200, refused("superseded"))
        );
        assert!(
            snapshot(&f).await? == before,
            "late first grant replaced the winning enrollment"
        );
        assert_eq!(
            f.rows("SELECT enrollment_generation FROM login_machines")
                .await?,
            vec![vec![number(1)]]
        );
        assert_eq!(
            f.rows("SELECT COUNT(*) FROM login_machine_approvals WHERE state='enrolled'")
                .await?,
            vec![vec![number(1)]]
        );
        f.advance(1_000).await?;
        assert_eq!(
            f.post(TOKEN, &token(winner, winner)).await?,
            (200, grant(winner, INSTALLATION, f.now(), None))
        );
        assert_eq!(
            f.rows("SELECT enrollment_generation FROM login_machines")
                .await?,
            vec![vec![number(1)]],
            "active repeat must not bump generation"
        );
        assert_eq!(
            f.rows("SELECT COUNT(*) FROM signin_change_records WHERE kind='machineEnrolled'")
                .await?,
            vec![vec![number(1)]]
        );
        f.close().await?;
    }
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "A newer-generation approval supersedes exactly one active login and retains distinct compound audit identities."
)]
#[tokio::test]
async fn a_new_approval_leaves_old_login_active_until_grant_then_supersedes_once() -> Result<()> {
    let f = MachineFixture::new().await?;
    let a = enrollment(
        &f.post(
            DEVICE,
            &approval(0xc501, INSTALLATION, "A", &[WORKSPACE], f.now()),
        )
        .await?
        .1,
    );
    assert_eq!(
        f.post(TOKEN, &token(&a, &a)).await?,
        (200, grant(&a, INSTALLATION, f.now(), None))
    );
    let b = enrollment(
        &f.post(
            DEVICE,
            &approval(0xc502, INSTALLATION, "B", &[WORKSPACE_TWO], f.now()),
        )
        .await?
        .1,
    );
    assert_eq!(
        f.post(TOKEN, &token(&a, &a)).await?,
        (200, grant(&a, INSTALLATION, f.now(), None)),
        "approval alone leaves old enrollment active"
    );
    assert_eq!(
        f.post(TOKEN, &token(&b, &b)).await?,
        (200, grant(&b, INSTALLATION, f.now(), Some((&a, &a))))
    );
    let before = snapshot(&f).await?;
    assert_eq!(
        f.post(TOKEN, &token(&a, &a)).await?,
        (200, refused("superseded"))
    );
    assert!(snapshot(&f).await? == before);
    let replay = f
        .post(
            DEVICE,
            &approval(
                0xc501,
                INSTALLATION,
                "Different after ended",
                &[WORKSPACE_TWO],
                f.now(),
            ),
        )
        .await?;
    assert_eq!(
        replay,
        (
            200,
            json!({"result":"approved","enrollmentId":a,"label":"A","allowedWorkspaces":[WORKSPACE]})
        ),
        "N03 retains the original snapshot after the enrollment ends"
    );
    assert!(
        snapshot(&f).await? == before,
        "ended approval replay changed rows"
    );
    assert_eq!(
        f.rows("SELECT state,enrollment_generation,allowed_workspaces FROM login_machines")
            .await?,
        vec![vec![
            text("enrolled"),
            number(2),
            text(serde_json::to_string(&vec![WORKSPACE, WORKSPACE_TWO])?)
        ]]
    );
    let records=f.rows("SELECT request_id,kind,origin,actor_person_id,permission_checked,target_id FROM signin_change_records WHERE kind IN ('machineRevoked','machineEnrolled') ORDER BY id").await?;
    assert_eq!(
        records,
        vec![
            vec![
                text(format!("{}:machineEnrolled:{a}", id(0xc501))),
                text("machineEnrolled"),
                text("person"),
                text(signed::OWNER),
                text("machine:enroll"),
                text(&a)
            ],
            vec![
                text(format!("{}:machineRevoked:{a}", id(0xc502))),
                text("machineRevoked"),
                text("person"),
                text(signed::OWNER),
                text("machine:enroll"),
                text(&a)
            ],
            vec![
                text(format!("{}:machineEnrolled:{b}", id(0xc502))),
                text("machineEnrolled"),
                text("person"),
                text(signed::OWNER),
                text("machine:enroll"),
                text(&b)
            ]
        ]
    );
    assert_eq!(
        f.rows("SELECT COUNT(*) FROM login_machine_approvals WHERE state='enrolled'")
            .await?,
        vec![vec![number(1)]]
    );
    f.close().await
}
