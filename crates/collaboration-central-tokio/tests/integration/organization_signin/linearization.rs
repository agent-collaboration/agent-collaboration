use super::support::*;
use collaboration_central_core::PausePoint;
use serde_json::json;
use std::time::Duration;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Pre-sample expiry assertions preserve the historical applying-time refusal boundary."
)]
#[tokio::test]
async fn pre_sample_holds_keep_first_replay_renewal_and_stored_expiry_assertions() -> Result {
    for (kind, release_after, expected) in [
        ("first", 2, 422),
        ("first", 1, 200),
        ("replay", 2, 422),
        ("renewal", 2, 422),
        ("stored", 2, 409),
        ("stored", 1, 200),
    ] {
        let f = Fixture::new().await?;
        let mut short_claims = signed::claims(A, signed::NOW_SECONDS);
        signed::set_claim(&mut short_claims, "exp", json!(signed::NOW_SECONDS + 2))?;
        let short = signed::sign(&signed::stamp_header(), &short_claims, 7)?;
        let long = signed::stamp(A, signed::NOW_SECONDS)?;
        let incoming = match kind {
            "replay" => {
                assert_eq!(f.provision(A, &short).await?.status, 200);
                short.clone()
            }
            "renewal" => {
                assert_eq!(f.provision(A, &long).await?.status, 200);
                signed::set_claim(&mut short_claims, "replaces", json!(signed::digest(&long)))?;
                signed::sign(&signed::stamp_header(), &short_claims, 7)?
            }
            "stored" => {
                assert_eq!(f.provision(A, &short).await?.status, 200);
                long
            }
            _ => short,
        };
        let before = f.snapshot().await?;
        let runtime = f.runtime(A).await?;
        let mut held = runtime
            .pause_points()
            .hold_central(PausePoint::BeforeSignInSample);
        let client = f.client();
        let body = serde_json::to_string(&provision_body(A, &incoming))?;
        let request = tokio::spawn(async move { post(&client, PROVISION, body, vec![]).await });
        held.until_held().await;
        f.clock.advance(Duration::from_secs(release_after))?;
        held.release();
        assert_eq!(
            request.await??.status,
            expected,
            "{kind}, release offset {release_after}"
        );
        let after = f.snapshot().await?;
        if kind == "first" {
            let digest = f.post(DIGEST, &json!({"organizationId":A})).await?;
            assert_eq!(digest.status, if expected == 200 { 200 } else { 404 });
            if expected != 200 {
                assert!(digest.body.is_empty());
            }
            assert_eq!(
                (after.0.len(), after.1.len()),
                if expected == 200 { (1, 1) } else { (0, 0) }
            );
        } else {
            assert!(
                after == before,
                "held {kind} must leave stored rows exactly unchanged"
            );
        }
        f.close().await?;
    }
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The causal hold proves a committed sample and a second writer waiting on the org turn."
)]
#[tokio::test]
async fn post_sample_clock_advance_commits_original_time_and_the_next_writer_queues() -> Result {
    let f = Fixture::new().await?;
    let runtime = f.runtime(A).await?;
    let mut claims = signed::claims(A, signed::NOW_SECONDS);
    signed::set_claim(&mut claims, "exp", json!(signed::NOW_SECONDS + 1))?;
    let statement = signed::sign(&signed::stamp_header(), &claims, 7)?;
    let mut held = runtime
        .pause_points()
        .hold_central(PausePoint::AfterSignInSample);
    let first_client = f.client();
    let first_body = serde_json::to_string(&provision_body(A, &statement))?;
    let first =
        tokio::spawn(async move { post(&first_client, PROVISION, first_body, vec![]).await });
    held.until_held().await;
    let second_client = f.client();
    let second_body = serde_json::to_string(&organization_floor(A, 1, "active"))?;
    let mut second =
        tokio::spawn(async move { post(&second_client, REVISIONS, second_body, vec![]).await });
    let waits = tokio::select! {()=runtime.until_central_waiting(1)=>true,_=&mut second=>false};
    assert!(
        waits,
        "the second writer crossed the first writer's org turn"
    );
    f.clock.advance(Duration::from_secs(2))?;
    held.release();
    assert_eq!(
        first.await??.status,
        200,
        "the valid applying sample remains the linearization point"
    );
    assert_eq!(
        second.await??.status,
        503,
        "the queued writer samples the later expired time"
    );
    assert_eq!(
        f.rows(
            A,
            "SELECT created_at,updated_at FROM organization_identity",
            vec![]
        )
        .await?,
        vec![vec![
            number(signed::NOW_SECONDS * 1000)?,
            number(signed::NOW_SECONDS * 1000)?
        ]]
    );
    assert_eq!(
        f.rows(
            A,
            "SELECT created_at,updated_at FROM organization_change_records",
            vec![]
        )
        .await?,
        vec![vec![
            number(signed::NOW_SECONDS * 1000)?,
            number(signed::NOW_SECONDS * 1000)?
        ]]
    );
    f.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Provisioning must activate the actual native alarm owner using its one injected clock."
)]
#[tokio::test]
async fn provisioning_the_first_stamp_activates_a_pending_wake_without_manual_alarm_calls() -> Result
{
    use collaboration_central_core::WorkspaceCaller;
    use collaboration_protocol::central_mail::{
        CentralMailAnswer, CentralMailOperation, CentralMailRequest, CentralMailResult,
    };
    use router_clock::RuntimeClock;
    let mut f = Fixture::new().await?;
    f.seed_machine().await?;
    let target = workspace_model::SessionAddress {
        machine_id: MACHINE.to_owned().try_into()?,
        endpoint: "codex-local".to_owned().try_into()?,
        session_id: "wake-session".to_owned().try_into()?,
    };
    let caller = WorkspaceCaller {
        organization_id: org(A)?,
        caller: workspace_model::Caller::Session(target.clone()),
        credential_hash: "signin-fixture-hash".into(),
    };
    let due = chrono::DateTime::from_timestamp(
        (f.clock.utc_now().timestamp().div_euclid(300) + 1) * 300,
        0,
    )
    .ok_or("wake grid mark exceeds timestamp range")?;
    let answer = f
        .host
        .request_central(
            &caller,
            CentralMailRequest {
                request_id: signed::fresh_request_id(signed::NOW_SECONDS)?.try_into()?,
                operation: CentralMailOperation::SetWake {
                    target,
                    due_at: due,
                    text: "pending before first stamp".to_owned().try_into()?,
                },
            },
        )
        .await?;
    let CentralMailResult::Done(CentralMailAnswer::WakeSet { wake_id, .. }) = answer else {
        return Err("unstamped wake setup refused".into());
    };
    let runtime = f.runtime(A).await?;
    assert_eq!(runtime.statement_host().alarm(), None);
    let mut alarm = runtime.statement_host().subscribe_alarm();
    assert_eq!(
        f.provision(A, &signed::stamp(A, signed::NOW_SECONDS)?)
            .await?
            .status,
        200
    );
    alarm.wait_for(|alarm| alarm.is_some()).await?;
    assert_eq!(
        *alarm.borrow_and_update(),
        Some(runtime_primitives::SyncTime::from_unix_millis(
            due.timestamp_millis()
        ))
    );
    f.clock
        .not_due_check_now(
            f.clock
                .wall_deadline(runtime_primitives::SyncTime::from_unix_millis(
                    due.timestamp_millis(),
                )),
        )
        .await;
    f.clock.advance((due - f.clock.utc_now()).to_std()?)?;
    alarm.wait_for(|alarm| alarm.is_none()).await?;
    assert_eq!(
        f.rows(
            A,
            "SELECT state,(SELECT COUNT(*) FROM messages) FROM wakes WHERE wake_id=?",
            vec![text(wake_id)]
        )
        .await?,
        vec![vec![text("done"), number(1)?]]
    );
    f.reopen().await?;
    assert_eq!(
        f.rows(A, "SELECT COUNT(*) FROM messages", vec![]).await?,
        vec![vec![number(1)?]],
        "restart does not duplicate the activated wake"
    );
    f.close().await
}
