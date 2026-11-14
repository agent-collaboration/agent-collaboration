//! Pre-sample holds preserve the six native statement-expiry assertions.
use super::support::{DIGEST, Fixture, ORG, PROVISION};
use central_storage::ReadOutcome;
use collaboration_central_conformance::signin_fixtures::{self as signed, NOW_SECONDS};
use serde_json::{Value, json};

async fn snapshot(f: &Fixture) -> (ReadOutcome, ReadOutcome) {
    (
        f.read("SELECT * FROM organization_identity ORDER BY id")
            .await,
        f.read("SELECT * FROM organization_change_records ORDER BY id")
            .await,
    )
}
async fn held_provision(f: &Fixture, incoming: &str, release_after_seconds: i64) -> (u16, Value) {
    let runtime = f.runtime().await;
    assert!(
        runtime
            .control::<bool>("hold-central", &"beforeSignInSample")
            .await
            .unwrap()
    );
    let body = json!({"organizationId":ORG,"organizationStatement":incoming});
    let release = async {
        assert!(
            runtime
                .control::<bool>("until-held-central", &"beforeSignInSample")
                .await
                .unwrap()
        );
        f.clock.set_utc(
            chrono::DateTime::from_timestamp(NOW_SECONDS + release_after_seconds, 0).unwrap(),
        );
        f.runtime().await;
        assert!(
            runtime
                .control::<bool>("release-central", &"beforeSignInSample")
                .await
                .unwrap()
        );
    };
    let (answer, ()) = tokio::join!(f.post(PROVISION, &body), release);
    answer
}

#[tokio::test]
async fn pre_sample_holds_keep_first_replay_renewal_and_stored_expiry_assertions() {
    for (kind, release_after, expected) in [
        ("first", 2, 422),
        ("first", 1, 200),
        ("replay", 2, 422),
        ("renewal", 2, 422),
        ("stored", 2, 409),
        ("stored", 1, 200),
    ] {
        let f = Fixture::new().await;
        let mut short_claims = signed::claims(ORG, NOW_SECONDS);
        signed::set_claim(&mut short_claims, "exp", json!(NOW_SECONDS + 2)).unwrap();
        let short = signed::sign(&signed::stamp_header(), &short_claims, 7).unwrap();
        let long = signed::stamp(ORG, NOW_SECONDS).unwrap();
        let incoming = match kind {
            "replay" => {
                assert_eq!(
                    f.post(
                        PROVISION,
                        &json!({"organizationId":ORG,"organizationStatement":short})
                    )
                    .await
                    .0,
                    200
                );
                short.clone()
            }
            "renewal" => {
                assert_eq!(
                    f.post(
                        PROVISION,
                        &json!({"organizationId":ORG,"organizationStatement":long})
                    )
                    .await
                    .0,
                    200
                );
                signed::set_claim(&mut short_claims, "replaces", json!(signed::digest(&long)))
                    .unwrap();
                signed::sign(&signed::stamp_header(), &short_claims, 7).unwrap()
            }
            "stored" => {
                assert_eq!(
                    f.post(
                        PROVISION,
                        &json!({"organizationId":ORG,"organizationStatement":short})
                    )
                    .await
                    .0,
                    200
                );
                long
            }
            _ => short,
        };
        let before = snapshot(&f).await;
        let answer = held_provision(&f, &incoming, release_after).await;
        assert_eq!(answer.0, expected, "{kind}, release offset {release_after}");
        let after = snapshot(&f).await;
        if kind == "first" {
            let (status, bytes) = f
                .post_bytes(DIGEST, &json!({"organizationId":ORG}), &[])
                .await;
            assert_eq!(status, if expected == 200 { 200 } else { 404 });
            if expected != 200 {
                assert!(bytes.is_empty());
            }
            assert_eq!(
                (after.0.rows.len(), after.1.rows.len()),
                if expected == 200 { (1, 1) } else { (0, 0) }
            );
        } else {
            // Opaque comparison prevents signed fixture material from entering assertion output.
            assert!(
                after == before,
                "held {kind} changed a retained identity or record"
            );
        }
        f.close().await;
    }
}
