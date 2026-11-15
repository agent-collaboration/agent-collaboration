use super::support::*;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Actual public Worker edge must refuse both private decision routes without changing their shared owning namespace."
)]
#[tokio::test]
async fn machine_decision_paths_remain_private_with_exact_public_401_classification() -> Result<()>
{
    let f = MachineFixture::new().await?;
    let first = f
        .post(
            DEVICE,
            &approval(0xcd01, INSTALLATION, "Private", &[WORKSPACE], f.now()),
        )
        .await?;
    let enrollment_id = enrollment(&first.1);
    assert_eq!(
        f.post(TOKEN, &token(&enrollment_id, &enrollment_id))
            .await?
            .0,
        200
    );
    let before = snapshot(&f).await?;
    let client = reqwest::Client::builder().no_proxy().build()?;
    let access = collaboration_central_conformance::owner_assertion()?;
    for (path, body) in [
        (
            DEVICE,
            approval(
                0xcd02,
                INSTALLATION,
                "Private changed",
                &[WORKSPACE_TWO],
                f.now(),
            ),
        ),
        (TOKEN, token(&enrollment_id, &enrollment_id)),
    ] {
        for present in [false, true] {
            let mut request = client
                .post(format!("{}{path}", f.inner.host.public_origin()))
                .json(&body);
            if present {
                request = request.header("Cf-Access-Jwt-Assertion", &access);
            }
            let response = request.send().await?;
            assert_eq!(response.status().as_u16(), 401);
            assert_eq!(
                response.json::<Value>().await?,
                json!({"kind":if present{"routeRefused"}else{"missingAssertion"}})
            );
            assert!(
                snapshot(&f).await? == before,
                "public decision changed actual private namespace machine state"
            );
        }
    }
    f.close().await
}
