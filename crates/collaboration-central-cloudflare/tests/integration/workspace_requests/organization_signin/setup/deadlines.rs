//! Held real Create operations retain B2 deadline effects and serialization oracles.
use super::{owned_rows, support::*};
use central_storage::SqlValue;

type Adjustment = fn(&mut Value);
fn token_bound(claims: &mut Value) {
    claims["exp"] = json!(NOW_SECONDS + 30);
    claims["session_expires_at_ms"] = json!((NOW_SECONDS + 60) * 1000);
    claims["authorization_snapshot_valid_until_ms"] = json!((NOW_SECONDS + 90) * 1000);
}
fn session_bound(claims: &mut Value) {
    claims["exp"] = json!(NOW_SECONDS + 90);
    claims["session_expires_at_ms"] = json!((NOW_SECONDS + 30) * 1000);
    claims["authorization_snapshot_valid_until_ms"] = json!((NOW_SECONDS + 60) * 1000);
}
fn snapshot_bound(claims: &mut Value) {
    claims["exp"] = json!(NOW_SECONDS + 90);
    claims["session_expires_at_ms"] = json!((NOW_SECONDS + 60) * 1000);
    claims["authorization_snapshot_valid_until_ms"] = json!((NOW_SECONDS + 30) * 1000);
}

#[tokio::test]
async fn signed_create_deadlines_refuse_at_bound_and_apply_before() {
    let cases: [(&str, Adjustment); 3] = [
        ("token exp", token_bound),
        ("session deadline", session_bound),
        ("snapshot deadline", snapshot_bound),
    ];
    for (bound, adjust) in cases {
        for release_after in [30_000, 29_999] {
            let f = ready().await;
            let owner = f.owner();
            let operation = operation(0x731);
            let (body, intent) = create(&operation, "Held create", "");
            let proof = owner
                .headers_with(
                    signed::CREATE,
                    &intent,
                    &signed::fresh_request_id(NOW_SECONDS).unwrap(),
                    NOW_SECONDS,
                    &adjust,
                )
                .unwrap();
            let before = owned_rows(&f).await;
            let runtime = f.runtime().await;
            assert!(
                runtime
                    .control::<bool>("hold-central", &"beforeSignInSample")
                    .await
                    .unwrap()
            );
            let release = async {
                assert!(
                    runtime
                        .control::<bool>("until-held-central", &"beforeSignInSample")
                        .await
                        .unwrap()
                );
                f.clock.set_utc(
                    chrono::DateTime::from_timestamp_millis(NOW_SECONDS * 1000 + release_after)
                        .unwrap(),
                );
                f.runtime().await;
                assert!(
                    runtime
                        .control::<bool>("release-central", &"beforeSignInSample")
                        .await
                        .unwrap()
                );
            };
            let (answer, ()) = tokio::join!(f.post_with(signed::CREATE, &body, &proof), release);
            assert_eq!(answer.0, 200);
            if release_after == 30_000 {
                assert_eq!(
                    answer.1,
                    json!({"result":"refused","reason":"expired"}),
                    "{bound}: released at the deadline"
                );
                assert!(
                    owned_rows(&f).await == before,
                    "{bound}: refusal must reserve, decide or write nothing"
                );
            } else {
                assert_eq!(
                    answer.1,
                    json!({"result":"applied","workspace":{"workspaceId":operation,"name":"Held create","description":"","state":"active"}}),
                    "{bound}: positive control actually creates one workspace before the deadline"
                );
                assert_eq!(f.read("SELECT (SELECT COUNT(*) FROM workspaces),(SELECT COUNT(*) FROM organization_change_records),(SELECT COUNT(*) FROM request_log),(SELECT COUNT(*) FROM organization_used_requests)").await.rows,vec![vec![number(1),number(2),number(1),number(1)]]);
            }
            f.close().await;
        }
    }
}

#[tokio::test]
async fn signed_create_post_sample_time_advance_keeps_commit_and_next_writer_waits() {
    let f = ready().await;
    let owner = f.owner();
    let first_id = operation(0x732);
    let (first_body, first_intent) = create(&first_id, "Sampled", "");
    let proof = owner
        .headers_with(
            signed::CREATE,
            &first_intent,
            &signed::fresh_request_id(NOW_SECONDS).unwrap(),
            NOW_SECONDS,
            &token_bound,
        )
        .unwrap();
    let runtime = f.runtime().await;
    assert!(
        runtime
            .control::<bool>("hold-central", &"afterSignInSample")
            .await
            .unwrap()
    );
    let origin = f.host.signin_origin();
    let first = tokio::spawn(async move {
        let mut request = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .post(format!("{origin}{}", signed::CREATE))
            .json(&first_body);
        for (name, value) in proof {
            request = request.header(name, value);
        }
        let response = request.send().await.unwrap();
        (response.status().as_u16(), response.bytes().await.unwrap())
    });
    assert!(
        runtime
            .control::<bool>("until-held-central", &"afterSignInSample")
            .await
            .unwrap()
    );
    let second_id = operation(0x733);
    let (second_body, second_intent) = create(&second_id, "Queued", "");
    let proof = headers(&owner, &second_intent, NOW_SECONDS);
    let origin = f.host.signin_origin();
    let mut second = tokio::spawn(async move {
        let mut request = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .post(format!("{origin}{}", signed::CREATE))
            .json(&second_body);
        for (name, value) in proof {
            request = request.header(name, value);
        }
        let response = request.send().await.unwrap();
        (response.status().as_u16(), response.bytes().await.unwrap())
    });
    let waits = tokio::select! {wait=runtime.control::<bool>("until-central-waiting",&1usize)=>wait.unwrap(),_=&mut second=>false};
    assert!(waits, "next create must wait for the sampled org turn");
    f.clock
        .set_utc(chrono::DateTime::from_timestamp(NOW_SECONDS + 30, 0).unwrap());
    f.runtime().await;
    assert!(
        runtime
            .control::<bool>("release-central", &"afterSignInSample")
            .await
            .unwrap()
    );
    let first = first.await.unwrap();
    assert_eq!(first.0, 200);
    let first: Value = serde_json::from_slice(&first.1).unwrap();
    assert_eq!(first["result"], "applied");
    let second = second.await.unwrap();
    assert_eq!(second.0, 200);
    let second: Value = serde_json::from_slice(&second.1).unwrap();
    assert_eq!(second["result"], "applied");
    let rows=f.read("SELECT created_at FROM organization_change_records WHERE kind='workspaceCreated' ORDER BY id").await.rows;
    assert_eq!(
        rows,
        vec![
            vec![number(NOW_SECONDS * 1000)],
            vec![number((NOW_SECONDS + 30) * 1000)]
        ],
        "each writer owns its own applying sample"
    );
    let workspace_rows = f
        .read("SELECT created_at FROM workspaces ORDER BY created_at")
        .await
        .rows;
    assert!(
        matches!(workspace_rows.as_slice(),[first,second]if matches!(first.first(),Some(SqlValue::Text(at))if at==&chrono::DateTime::from_timestamp(NOW_SECONDS,0).unwrap().to_rfc3339())&&matches!(second.first(),Some(SqlValue::Text(at))if at==&chrono::DateTime::from_timestamp(NOW_SECONDS+30,0).unwrap().to_rfc3339()))
    );
    f.close().await;
}
