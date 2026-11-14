use super::support::*;
use collaboration_central_tokio::{TokioServer, TokioSettings};
use std::time::Duration;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Actual public edge must refuse both private machine-decision paths with or without an Access assertion."
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
    let server = TokioServer::start_with_clock(
        TokioSettings {
            root: f.inner.root.path().join("public"),
            access_policy: collaboration_central_conformance::fixture_policy(),
            jwks_url: f.inner.keys.url.clone(),
            registry: crate::configuration::registry_config()?,
            delivery: collaboration_central_tokio::DeliveryConfig {
                relay_peer: "019f0000-0000-7000-8000-000000005240"
                    .to_owned()
                    .try_into()?,
            },
            holder_answer_timeout: Duration::from_secs(2),
            copy_ack_wait: Duration::from_secs(2),
        },
        f.inner.clock.clone(),
    )
    .await?;
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
                .post(format!("{}{path}", server.base_url()))
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
                "public machine decision changed private owning state"
            );
        }
    }
    server.shutdown().await?;
    f.close().await
}
