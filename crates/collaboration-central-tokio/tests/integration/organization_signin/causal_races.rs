use super::support::*;
use collaboration_central_core::PausePoint;
#[expect(
    clippy::panic_in_result_fn,
    reason = "Real private-link calls and a causal pre-sample hold determine exactly which first stamp commits."
)]
#[tokio::test]
async fn a_second_first_provision_commits_while_the_first_waits_and_identical_retries_keep_one_record()
-> Result {
    for identical in [false, true] {
        let f = Fixture::new().await?;
        let first = signed::stamp(A, signed::NOW_SECONDS)?;
        let second = if identical {
            first.clone()
        } else {
            signed::first_statement(A, signed::NOW_SECONDS, 1)?
        };
        let runtime = f.runtime(A).await?;
        let mut held = runtime
            .pause_points()
            .hold_central(PausePoint::BeforeSignInSample);
        let client = f.client();
        let body = serde_json::to_string(&provision_body(A, &first))?;
        let waiting = tokio::spawn(async move { post(&client, PROVISION, body, vec![]).await });
        held.until_held().await;
        assert_eq!(f.provision(A, &second).await?.status, 200);
        let committed = f.snapshot().await?;
        assert_eq!((committed.0.len(), committed.1.len()), (1, 1));
        held.release();
        assert_eq!(waiting.await??.status, 200);
        assert!(
            f.snapshot().await? == committed,
            "the released first caller must not replace or duplicate the known winner"
        );
        assert!(
            f.rows(
                A,
                "SELECT organization_statement FROM organization_identity",
                vec![]
            )
            .await?[0][0]
                == text(&second)
        );
        f.close().await?;
    }
    Ok(())
}
#[expect(
    clippy::panic_in_result_fn,
    reason = "The known committed renewal makes the held predecessor stale without changing stamp or record state."
)]
#[tokio::test]
async fn a_held_renewal_loses_to_the_committed_second_renewal_without_new_effects() -> Result {
    let f = Fixture::new().await?;
    let original = signed::stamp(A, signed::NOW_SECONDS)?;
    assert_eq!(f.provision(A, &original).await?.status, 200);
    let first = signed::renewal(A, &original, signed::NOW_SECONDS, 1)?;
    let second = signed::renewal(A, &original, signed::NOW_SECONDS, 2)?;
    let runtime = f.runtime(A).await?;
    let mut held = runtime
        .pause_points()
        .hold_central(PausePoint::BeforeSignInSample);
    let client = f.client();
    let body = serde_json::to_string(&provision_body(A, &first))?;
    let waiting = tokio::spawn(async move { post(&client, PROVISION, body, vec![]).await });
    held.until_held().await;
    assert_eq!(f.provision(A, &second).await?.status, 200);
    let committed = f.snapshot().await?;
    assert_eq!(committed.1.len(), 2);
    held.release();
    assert_eq!(waiting.await??.status, 409);
    assert!(
        f.snapshot().await? == committed,
        "stale renewal must not add a record or overwrite the winner"
    );
    assert!(
        f.rows(
            A,
            "SELECT organization_statement FROM organization_identity",
            vec![]
        )
        .await?[0][0]
            == text(second)
    );
    f.close().await
}
