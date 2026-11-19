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
    let a = enrollment(&first.1)?;
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
    let b = enrollment(&next.1)?;
    // Execute activation, then retry as an issuer that did not retain its reply.
    // No fixture transport-loss injection is claimed by this request/retry case.
    let first_grant = f.post(TOKEN, &token(&b, &b)).await?;
    assert_eq!(
        first_grant,
        (200, grant(&b, INSTALLATION, f.now(), Some((&a, &a))))
    );
    let activation = f.rows("SELECT approval_id,state,machine_client_id,base_generation FROM login_machine_approvals ORDER BY approval_id").await?;
    let generation = f
        .rows("SELECT enrollment_generation FROM login_machines")
        .await?;
    let records = f
        .rows("SELECT * FROM signin_change_records ORDER BY id")
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
        assert!(f.rows("SELECT approval_id,state,machine_client_id,base_generation FROM login_machine_approvals ORDER BY approval_id").await? == activation);
        assert!(
            f.rows("SELECT enrollment_generation FROM login_machines")
                .await?
                == generation
        );
        assert!(
            f.rows("SELECT * FROM signin_change_records ORDER BY id")
                .await?
                == records,
            "notice recovery must not emit another revocation or activation"
        );
    }
    // Valid scalar domains can still encode a false predecessor association.
    // Corruption below is setup; the real route must refuse without effects.
    for (damage, repair) in [
        (
            "UPDATE signin_change_records SET permission_checked='machine:revoke' WHERE kind='machineRevoked'",
            "UPDATE signin_change_records SET permission_checked='machine:enroll' WHERE kind='machineRevoked'",
        ),
        (
            "UPDATE login_machine_approvals SET end_reason='owner' WHERE end_reason='superseded'",
            "UPDATE login_machine_approvals SET end_reason='superseded' WHERE end_reason='owner'",
        ),
    ] {
        let healthy = snapshot(&f).await?;
        f.execute(damage, vec![]).await?;
        let damaged = snapshot(&f).await?;
        assert!(damaged != healthy);
        assert_eq!(f.post(TOKEN, &token(&b, &b)).await?, (503, Value::Null));
        assert!(snapshot(&f).await? == damaged);
        f.execute(repair, vec![]).await?;
        assert!(snapshot(&f).await? == healthy);
        assert_eq!(
            f.post(TOKEN, &token(&b, &b)).await?,
            (200, grant(&b, INSTALLATION, f.now(), Some((&a, &a))))
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

#[expect(
    clippy::panic_in_result_fn,
    reason = "Two individually valid predecessor records must refuse through the actual route without changing durable rows."
)]
#[tokio::test]
async fn two_valid_supersession_predecessors_refuse_without_effects_then_repair_restores_notice()
-> Result<()> {
    // Arrange: A, B and C activate at one injected time. A and B therefore both
    // have valid superseded state, client, origin and end/record timestamps.
    let f = MachineFixture::new().await?;
    let activation_time = f.now();
    let mut previous: Option<String> = None;
    let mut activated = Vec::new();
    for (serial, label) in [
        (0xe501, "Notice A"),
        (0xe502, "Notice B"),
        (0xe503, "Notice C"),
    ] {
        let approved = f
            .post(
                DEVICE,
                &approval(serial, INSTALLATION, label, &[WORKSPACE], f.now()),
            )
            .await?;
        assert_eq!(approved.0, 200);
        let current = enrollment(&approved.1)?;
        assert_eq!(
            f.post(TOKEN, &token(&current, &current)).await?,
            (
                200,
                grant(
                    &current,
                    INSTALLATION,
                    activation_time,
                    previous.as_ref().map(|id| (id.as_str(), id.as_str())),
                )
            )
        );
        previous = Some(current.clone());
        activated.push(current);
    }
    assert_eq!(f.now(), activation_time);
    let a = &activated[0];
    let b = &activated[1];
    let c = &activated[2];
    assert_eq!(
        f.rows("SELECT state,end_reason FROM login_machine_approvals WHERE label IN ('Notice A','Notice B') ORDER BY label").await?,
        vec![vec![text("ended"), text("superseded")], vec![text("ended"), text("superseded")]]
    );
    assert_eq!(
        f.rows("SELECT approval_id FROM login_machine_approvals WHERE state='enrolled'")
            .await?,
        vec![vec![text(c)]]
    );
    let healthy = snapshot(&f).await?;
    let seeded_id = id(0xe50f);
    // Seed only the ambiguous association: clone A's real revocation record
    // (created by B), preserving every validated scalar and timestamp.
    f.execute(
        "INSERT INTO signin_change_records(id,request_id,kind,origin,actor_person_id,permission_checked,target_id,created_at,updated_at) SELECT ?,?||':machineRevoked:'||target_id,kind,origin,actor_person_id,permission_checked,target_id,created_at,updated_at FROM signin_change_records WHERE kind='machineRevoked' AND request_id=? AND target_id=?",
        vec![text(&seeded_id), text(id(0xe503)), text(format!("{}:machineRevoked:{a}", id(0xe502))), text(a)],
    ).await?;
    assert_eq!(
        f.rows("SELECT COUNT(*) FROM signin_change_records WHERE kind='machineRevoked' AND request_id='019f0000-0000-7000-8000-00000000e503:machineRevoked:'||target_id").await?,
        vec![vec![number(2)?]]
    );
    let ambiguous = snapshot(&f).await?;
    assert!(ambiguous != healthy);

    // Act/Assert: selecting either candidate would grant. The cardinality guard
    // must instead produce 503 and a parsed null response, with all four stores intact.
    assert_eq!(f.post(TOKEN, &token(c, c)).await?, (503, Value::Null));
    assert!(
        snapshot(&f).await? == ambiguous,
        "ambiguous predecessor history changed machine, approvals, records or credentials"
    );

    // Delete exactly the seeded row, restoring the real chain. C's live retry
    // again names B, proving neither candidate was merely malformed or missing.
    f.execute(
        "DELETE FROM signin_change_records WHERE id=?",
        vec![text(&seeded_id)],
    )
    .await?;
    assert!(snapshot(&f).await? == healthy);
    assert_eq!(
        f.post(TOKEN, &token(c, c)).await?,
        (200, grant(c, INSTALLATION, activation_time, Some((b, b))))
    );
    f.close().await
}
