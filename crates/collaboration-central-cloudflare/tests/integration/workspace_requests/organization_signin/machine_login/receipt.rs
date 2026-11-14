use super::support::*;

#[expect(
    clippy::panic_in_result_fn,
    reason = "SD8 is measured after a causally held receipt, with equality accepted and the next millisecond refused."
)]
#[tokio::test]
async fn receipt_window_is_exactly_five_seconds_and_refuses_after_the_pre_sample_hold() -> Result<()>
{
    for millis in [5_000, 5_001] {
        let f = MachineFixture::new().await?;
        let request = approval(0xcb01, INSTALLATION, "Held receipt", &[WORKSPACE], f.now());
        let before = snapshot(&f).await?;
        let answer = f.held_approval(&request, millis).await?;
        assert_eq!(answer.0, 200);
        if millis == 5_000 {
            enrollment(&answer.1);
        } else {
            assert_eq!(
                answer.1,
                json!({"result":"refused","reason":{"kind":"notPermitted"}})
            );
            assert!(
                snapshot(&f).await? == before,
                "SD8 refusal wrote a machine, approval, allowance or record"
            );
        }
        f.close().await?;
    }
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Unknown and unstamped decision paths fail closed without synthetic approvals or token grants."
)]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Actual unknown and unstamped route refusals must preserve owning machine state."
)]
#[tokio::test]
async fn unknown_enrollment_and_foreign_organization_requests_do_not_create_machine_facts()
-> Result<()> {
    let f = MachineFixture::new().await?;
    let before = snapshot(&f).await?;
    assert_eq!(
        f.post(TOKEN, &token(&id(0xcb99), &id(0xcb99))).await?,
        (200, refused("enrollmentNotActive"))
    );
    assert!(snapshot(&f).await? == before);
    let mut foreign = approval(
        0xcb02,
        INSTALLATION,
        "Foreign unstamped",
        &[WORKSPACE],
        f.now(),
    );
    foreign["organizationId"] = json!(OTHER_ORG);
    assert_eq!(f.post(DEVICE, &foreign).await?, (503, Value::Null));
    let mut foreign_token = token(&id(0xcb99), &id(0xcb99));
    foreign_token["organizationId"] = json!(OTHER_ORG);
    assert_eq!(f.post(TOKEN, &foreign_token).await?, (503, Value::Null));
    assert!(
        snapshot(&f).await? == before,
        "an unstamped org decision changed the existing org"
    );
    f.close().await
}
