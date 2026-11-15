use super::support::*;
use collaboration_central_core::PausePoint;
use serde_json::{Value, json};
use std::time::Duration;

fn deadline(claims: &mut Value, bound: &str) {
    if let Some(claims) = claims.as_object_mut() {
        claims.insert(
            "exp".into(),
            json!(signed::NOW_SECONDS + if bound == "token" { 30 } else { 90 }),
        );
        claims.insert(
            "session_expires_at_ms".into(),
            json!((signed::NOW_SECONDS + if bound == "session" { 30 } else { 60 }) * 1000),
        );
        claims.insert(
            "authorization_snapshot_valid_until_ms".into(),
            json!((signed::NOW_SECONDS + if bound == "snapshot" { 30 } else { 60 }) * 1000),
        );
    }
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The actual signed Create must apply only before each independent signed deadline."
)]
#[tokio::test]
async fn signed_create_deadlines_refuse_at_bound_and_apply_before() -> Result {
    for bound in ["token", "session", "snapshot"] {
        for (advance, expired) in [(30_000, true), (29_999, false)] {
            let f = Fixture::new().await?;
            f.ready().await?;
            let person = signed::Person::owner(A, signed::NOW_SECONDS)?;
            let operation = "019f0000-0000-7000-8000-00000000d761";
            let body = serde_json::to_string(&body(operation, "Deadline", "held create"))?;
            let headers = person.headers_with(
                signed::CREATE,
                &intent(operation, "Deadline", "held create"),
                &signed::fresh_request_id(signed::NOW_SECONDS)?,
                signed::NOW_SECONDS,
                &|claims| deadline(claims, bound),
            )?;
            let runtime = f.runtime(A).await?;
            let mut held = runtime
                .pause_points()
                .hold_central(PausePoint::BeforeSignInSample);
            let before = all_effects(&f).await?;
            let client = f.client();
            let pending =
                tokio::spawn(async move { post(&client, signed::CREATE, body, headers).await });
            held.until_held().await;
            f.clock.advance(Duration::from_millis(advance))?;
            held.release();
            let reply = pending.await??;
            assert_eq!(reply.status, 200, "{bound}");
            if expired {
                assert_eq!(
                    reply.value()?,
                    json!({"result":"refused","reason":"expired"}),
                    "{bound}"
                );
                assert!(
                    all_effects(&f).await? == before,
                    "{bound}: expired Create reserved or changed rows"
                );
            } else {
                assert_eq!(
                    reply.value()?,
                    json!({"result":"applied","workspace":{"workspaceId":operation,"name":"Deadline","description":"held create","state":"active"}}),
                    "{bound}"
                );
                assert_eq!(f.rows(A,"SELECT (SELECT COUNT(*) FROM workspaces),(SELECT COUNT(*) FROM request_log),(SELECT COUNT(*) FROM organization_change_records),(SELECT COUNT(*) FROM organization_used_requests)",vec![]).await?,vec![vec![number(1)?,number(1)?,number(2)?,number(1)?]]);
                let at = signed::NOW_SECONDS * 1000 + 29_999;
                assert_eq!(f.rows(A,"SELECT created_at,updated_at FROM organization_change_records WHERE kind='workspaceCreated'",vec![]).await?,vec![vec![number(at)?,number(at)?]]);
            }
            f.close().await?;
        }
    }
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "A causal post-sample hold keeps the original decision and visibly queues the competing org writer."
)]
#[tokio::test]
async fn sampled_create_commits_original_time_while_revocation_waits_on_org_turn() -> Result {
    let f = Fixture::new().await?;
    f.ready().await?;
    let owner = signed::Person::owner(A, signed::NOW_SECONDS)?;
    let operation = "019f0000-0000-7000-8000-00000000d762";
    let body = serde_json::to_string(&body(operation, "Sampled create", ""))?;
    let headers = owner.headers_with(
        signed::CREATE,
        &intent(operation, "Sampled create", ""),
        &signed::fresh_request_id(signed::NOW_SECONDS)?,
        signed::NOW_SECONDS,
        &|claims| deadline(claims, "token"),
    )?;
    let runtime = f.runtime(A).await?;
    let mut held = runtime
        .pause_points()
        .hold_central(PausePoint::AfterSignInSample);
    let client = f.client();
    let first = tokio::spawn(async move { post(&client, signed::CREATE, body, headers).await });
    held.until_held().await;
    let client = f.client();
    let revocation = serde_json::to_string(&person_floor(
        A,
        signed::OWNER,
        signed::OWNER_SUBJECT,
        5,
        "inactive",
    ))?;
    let mut second =
        tokio::spawn(async move { post(&client, REVISIONS, revocation, vec![]).await });
    let waiting = tokio::select! {()=runtime.until_central_waiting(1)=>true,_=&mut second=>false};
    assert!(waiting, "a second writer crossed the held org turn");
    f.clock.advance(Duration::from_secs(31))?;
    held.release();
    let applied = first.await??;
    assert_eq!(applied.status, 200);
    assert_eq!(applied.value()?.get("result"), Some(&json!("applied")));
    assert_eq!(second.await??.status, 200);
    assert_eq!(f.rows(A,"SELECT created_at,updated_at FROM organization_change_records WHERE kind='workspaceCreated'",vec![]).await?,vec![vec![number(signed::NOW_SECONDS*1000)?,number(signed::NOW_SECONDS*1000)?]]);
    let before = all_effects(&f).await?;
    let refused = create(&f, &owner, operation, "Sampled create", "").await?;
    assert_eq!(refused.status, 200);
    assert_eq!(
        refused.value()?,
        json!({"result":"refused","reason":"notPermitted"})
    );
    assert!(all_effects(&f).await? == before);
    f.close().await
}
