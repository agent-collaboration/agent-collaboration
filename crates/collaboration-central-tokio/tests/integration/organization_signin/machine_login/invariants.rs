//! Bounded sequence enumeration over the real owning routes and revoke seam.
//! This supplements, rather than replaces, each named BA1 edge scenario.
use super::support::*;

struct SequenceState {
    generation: i64,
    active: Option<String>,
}
async fn assert_invariant(f: &MachineFixture, state: &SequenceState, step: &str) -> Result<()> {
    let active = f
        .rows(
            "SELECT approval_id FROM machine_approvals WHERE state='enrolled' ORDER BY approval_id",
        )
        .await?;
    assert!(
        active.len() <= 1,
        "{step}: more than one enrolled login for the machine_id"
    );
    let expected = state
        .active
        .as_ref()
        .map(|id| vec![vec![text(id)]])
        .unwrap_or_default();
    assert_eq!(
        active, expected,
        "{step}: owning active enrollment differs from sequence model"
    );
    assert_eq!(
        f.rows("SELECT enrollment_generation FROM machines").await?,
        vec![vec![number(state.generation)?]],
        "{step}: generation drift"
    );
    Ok(())
}
async fn stale_grant(f: &MachineFixture, enrollment: &str, state: &SequenceState) -> Result<()> {
    let rows = f
        .rows(
            "SELECT approval_id,base_generation,state FROM machine_approvals ORDER BY approval_id",
        )
        .await?;
    let row = rows
        .iter()
        .find(|row| row.first() == Some(&text(enrollment)))
        .ok_or("stale approval absent")?;
    assert_ne!(
        row.get(1),
        Some(&number(state.generation)?),
        "the scenario must actually cross a generation before a pending Grant"
    );
    assert_eq!(
        row.get(2),
        Some(&text("approved")),
        "stale path must remain a never-activated approval"
    );
    let before = snapshot(f).await?;
    assert_eq!(
        f.post(TOKEN, &token(enrollment, enrollment)).await?,
        (200, refused("superseded"))
    );
    assert!(
        snapshot(f).await? == before,
        "a stale pending approval mutated state"
    );
    assert_invariant(f, state, "stale pending grant").await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Every bounded sequence step compares real SQL state with an independent generation/active-login model."
)]
#[tokio::test]
async fn bounded_three_login_sequences_preserve_one_enrolled_and_fence_every_stale_base_generation()
-> Result<()> {
    // Both first-winner orders cross three real operation interleavings after
    // replacement: stale pending Grant, old-client repeat and owner revocation.
    // No random source, retry or timing search is used.
    const ORDERS: [[u8; 3]; 3] = [[0, 1, 2], [0, 2, 1], [2, 1, 0]];
    for first_slot in [0_u16, 1] {
        for order in ORDERS {
            let f = MachineFixture::new().await?;
            let stale_slot = 1 - first_slot;
            let new_slot = 2;
            let mut state = SequenceState {
                generation: 0,
                active: None,
            };
            let mut approved = Vec::new();
            for slot in [0_u16, 1] {
                let answer = f
                    .post(
                        DEVICE,
                        &approval(
                            0xd100 + slot,
                            INSTALLATION,
                            &format!("Login {slot}"),
                            &[WORKSPACE],
                            f.now(),
                        ),
                    )
                    .await?;
                let enrollment_id = enrollment(&answer.1)?;
                approved.push((slot, enrollment_id));
                assert_invariant(&f, &state, "initial approval").await?;
            }
            let first = approved
                .iter()
                .find(|(slot, _)| *slot == first_slot)
                .unwrap()
                .1
                .clone();
            let stale = approved
                .iter()
                .find(|(slot, _)| *slot == stale_slot)
                .unwrap()
                .1
                .clone();
            assert_eq!(
                f.post(TOKEN, &token(&first, &first)).await?,
                (200, grant(&first, INSTALLATION, f.now(), None))
            );
            state.generation = 1;
            state.active = Some(first.clone());
            assert_invariant(&f, &state, "first activation").await?;
            assert_eq!(
                f.post(TOKEN, &token(&first, &first)).await?,
                (200, grant(&first, INSTALLATION, f.now(), None))
            );
            assert_invariant(&f, &state, "active repeat").await?;
            stale_grant(&f, &stale, &state).await?;
            let newer = enrollment(
                &f.post(
                    DEVICE,
                    &approval(
                        0xd100 + new_slot,
                        INSTALLATION,
                        &format!("Login {new_slot}"),
                        &[WORKSPACE],
                        f.now(),
                    ),
                )
                .await?
                .1,
            )?;
            assert_invariant(&f, &state, "new approval preserves current login").await?;
            assert_eq!(
                f.post(TOKEN, &token(&newer, &newer)).await?,
                (
                    200,
                    grant(&newer, INSTALLATION, f.now(), Some((&first, &first)))
                )
            );
            state.generation = 2;
            state.active = Some(newer.clone());
            assert_invariant(&f, &state, "replacement activation").await?;
            for action in order {
                match action {
                    0 => stale_grant(&f, &stale, &state).await?,
                    1 => {
                        let before = snapshot(&f).await?;
                        assert_eq!(
                            f.post(TOKEN, &token(&first, &first)).await?,
                            (200, refused("superseded"))
                        );
                        assert!(snapshot(&f).await? == before);
                        assert_invariant(&f, &state, "old active repeat").await?;
                    }
                    _ => {
                        let ended = f
                            .revoke(0xd200 + new_slot, INSTALLATION, &newer)
                            .await??
                            .ok_or("modeled active revoke returned none")?;
                        assert_eq!(
                            serde_json::to_value(ended)?,
                            json!({"enrollmentId":newer,"clientId":newer})
                        );
                        state.generation = 3;
                        state.active = None;
                        assert_invariant(&f, &state, "owner revocation").await?;
                    }
                }
            }
            stale_grant(&f, &stale, &state).await?;
            let before = snapshot(&f).await?;
            assert_eq!(
                f.post(TOKEN, &token(&newer, &newer)).await?,
                (200, refused("enrollmentNotActive"))
            );
            assert!(snapshot(&f).await? == before);
            assert_invariant(&f, &state, "revoked repeat").await?;
            let replay = f
                .post(
                    DEVICE,
                    &approval(
                        0xd100 + stale_slot,
                        INSTALLATION,
                        "changed submitted snapshot",
                        &[WORKSPACE_TWO],
                        f.now(),
                    ),
                )
                .await?;
            assert_eq!(
                replay,
                (
                    200,
                    json!({"result":"approved","enrollmentId":stale,"label":format!("Login {stale_slot}"),"allowedWorkspaces":[WORKSPACE]})
                )
            );
            assert!(snapshot(&f).await? == before);
            assert_invariant(&f, &state, "approval replay after ABA").await?;
            f.close().await?;
        }
    }
    Ok(())
}
