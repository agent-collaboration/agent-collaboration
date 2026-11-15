//! A retry must retain the predecessor notice from durable activation history.
use super::support::*;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Exact repeated supersession notices, current grant times and unchanged durable identities must survive owning reopen."
)]
#[tokio::test]
async fn active_grant_retry_reconstructs_the_same_superseded_client_after_reopen() -> Result<()> {
    let mut f = MachineFixture::new().await?;
    let first = f
        .post(
            DEVICE,
            &approval(0xe301, INSTALLATION, "Predecessor", &[WORKSPACE], f.now()),
        )
        .await?;
    let a = enrollment(&first.1);
    assert_eq!(
        f.post(TOKEN, &token(&a, &a)).await?,
        (200, grant(&a, INSTALLATION, f.now(), None))
    );
    let next = f
        .post(
            DEVICE,
            &approval(
                0xe302,
                INSTALLATION,
                "Replacement",
                &[WORKSPACE_TWO],
                f.now(),
            ),
        )
        .await?;
    let b = enrollment(&next.1);
    // Execute activation, then retry as an issuer that did not retain its reply.
    // No fixture transport-loss injection is claimed by this request/retry case.
    let first_grant = f.post(TOKEN, &token(&b, &b)).await?;
    assert_eq!(
        first_grant,
        (200, grant(&b, INSTALLATION, f.now(), Some((&a, &a))))
    );
    let activation = f.rows("SELECT approval_id,state,machine_client_id,base_generation FROM machine_approvals ORDER BY approval_id").await?;
    let generation = f.rows("SELECT enrollment_generation FROM machines").await?;
    let records = f
        .rows("SELECT * FROM organization_change_records ORDER BY id")
        .await?;
    for reopen in [false, true] {
        if reopen {
            f.reopen().await?;
        }
        f.advance(1_000).await?;
        assert_eq!(
            f.post(TOKEN, &token(&b, &b)).await?,
            (200, grant(&b, INSTALLATION, f.now(), Some((&a, &a)))),
            "active retry must retain the exact ended predecessor across reopen"
        );
        assert!(f.rows("SELECT approval_id,state,machine_client_id,base_generation FROM machine_approvals ORDER BY approval_id").await? == activation);
        assert!(f.rows("SELECT enrollment_generation FROM machines").await? == generation);
        assert!(
            f.rows("SELECT * FROM organization_change_records ORDER BY id")
                .await?
                == records,
            "notice recovery must not emit another revocation or activation"
        );
    }
    let before = snapshot(&f).await?;
    assert_eq!(
        f.post(TOKEN, &token(&a, &a)).await?,
        (200, refused("superseded"))
    );
    assert!(snapshot(&f).await? == before);
    f.close().await
}
