use super::support::*;

#[expect(
    clippy::panic_in_result_fn,
    reason = "A causally held first Grant commits at its sampled time while another same-generation Grant queues and loses."
)]
#[tokio::test]
async fn concurrent_same_generation_grants_keep_the_held_first_winner_and_one_generation_bump()
-> Result<()> {
    let f = MachineFixture::new().await?;
    let a = enrollment(
        &f.post(
            DEVICE,
            &approval(0xcc01, INSTALLATION, "First", &[WORKSPACE], f.now()),
        )
        .await?
        .1,
    )?;
    let b = enrollment(
        &f.post(
            DEVICE,
            &approval(0xcc02, INSTALLATION, "Second", &[WORKSPACE_TWO], f.now()),
        )
        .await?
        .1,
    )?;
    let sampled = f.now();
    let (first, second) = f.racing_grants(&token(&a, &a), &token(&b, &b)).await?;
    assert_eq!(first, (200, grant(&a, INSTALLATION, sampled, None)));
    assert_eq!(second, (200, refused("superseded")));
    assert_eq!(
        f.rows("SELECT enrollment_generation FROM login_machines")
            .await?,
        vec![vec![number(1)?]]
    );
    assert_eq!(
        f.rows("SELECT approval_id FROM login_machine_approvals WHERE state='enrolled'")
            .await?,
        vec![vec![text(&a)]]
    );
    assert_eq!(
        f.rows(
            "SELECT created_at,updated_at FROM signin_change_records WHERE kind='machineEnrolled'"
        )
        .await?,
        vec![vec![number(sampled * 1000)?, number(sampled * 1000)?]]
    );
    f.close().await
}
