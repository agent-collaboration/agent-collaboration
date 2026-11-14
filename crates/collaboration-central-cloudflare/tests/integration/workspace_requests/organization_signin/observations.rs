use super::support::*;
use collaboration_central_conformance::signin_fixtures::{self as signed, NOW_SECONDS};
use serde_json::json;
#[tokio::test]
async fn refused_statement_emits_one_closed_observation_and_no_statement_material() {
    let f = Fixture::new().await;
    let statement = signed::stamp(ORG, NOW_SECONDS).unwrap();
    let invalid = signed::tamper_signature(&statement).unwrap();
    let result = f
        .post(
            PROVISION,
            &json!({"organizationId":ORG,"organizationStatement":invalid}),
        )
        .await;
    assert_eq!(result.0, 422);
    f.host.until_log("refusal=BadSignature").await;
    let logs = f.host.captured_logs().join("\n");
    let events = logs
        .lines()
        .filter(|line| line.contains("central organization:") && line.contains(ORG))
        .count();
    assert_eq!(events, 1);
    for material in [
        &statement,
        &invalid,
        &signed::digest(&statement),
        &signed::digest(&invalid),
    ] {
        assert!(
            !logs.contains(material),
            "observation leaked statement material"
        );
    }
    for part in statement.split('.').chain(invalid.split('.')) {
        assert!(!logs.contains(part), "observation leaked compact segment");
    }
    assert!(!logs.contains("stamp-a"), "observation leaked key id");
    assert_eq!(f.provision().await.0, 200);
    assert_eq!(
        f.host
            .captured_logs()
            .iter()
            .filter(|line| line.contains("central organization:") && line.contains(ORG))
            .count(),
        1
    );
    f.close().await;
}

fn assert_safe_logs(logs: &str, compacts: &[String], label: &str) {
    for compact in compacts {
        assert!(
            !logs.contains(compact),
            "{label}: log leaked compact credential"
        );
        for segment in compact.split('.').filter(|part| part.len() > 8) {
            assert!(
                !logs.contains(segment),
                "{label}: log leaked a signed segment"
            );
        }
        assert!(
            !logs.contains(&signed::digest(compact)),
            "{label}: log leaked credential digest"
        );
    }
    for key_id in ["stamp-a", "stamp-b", "stamp-z", "unreachable-key"] {
        assert!(
            !logs.contains(key_id),
            "{label}: log leaked a key identifier"
        );
    }
}
#[tokio::test]
async fn every_historical_profile_refusal_emits_one_closed_kind_with_empty_body_and_no_writes() {
    // Keep real cold verification for both a known key and an unpublished key.
    for index in [0, 6] {
        let f = Fixture::new().await;
        assert_statement_refusal(&f, index, "cold issuer-key cache").await;
        f.close().await;
    }
    // Refusals leave the organization unprovisioned; only this fixture's key cache is shared.
    let f = Fixture::new().await;
    for index in [1, 2, 3, 4, 5, 7, 8, 9, 10, 11, 12] {
        assert_statement_refusal(&f, index, "shared issuer-key cache").await;
    }
    f.close().await;
}

async fn assert_statement_refusal(f: &Fixture, index: usize, cache: &str) {
    let case = signed::statement_refusal_case(index, ORG, NOW_SECONDS)
        .unwrap_or_else(|error| panic!("statement refusal vector {index}: {error}"));
    let label = format!("vector {index} ({}, {cache})", case.name);
    let is_org_event =
        |line: &&String| line.contains("central organization:") && line.contains(ORG);
    let before = f.host.captured_logs();
    let log_baseline = before.len();
    let event_baseline = before.iter().filter(is_org_event).count();
    let reply = f
        .post_bytes(
            PROVISION,
            &json!({"organizationId":ORG,"organizationStatement":case.statement}),
            &[],
        )
        .await;
    assert_eq!(reply.0, case.status, "{label}: wrong status");
    assert!(
        reply.1.is_empty(),
        "{label}: refusal must have an empty body"
    );
    f.host
        .until_log_after(log_baseline, "central organization:")
        .await;
    let after = f.host.captured_logs();
    let events = after
        .iter()
        .skip(log_baseline)
        .filter(is_org_event)
        .collect::<Vec<_>>();
    assert_eq!(
        events.len(),
        1,
        "{label}: each refused input must emit once"
    );
    assert_eq!(
        after.iter().filter(is_org_event).count(),
        event_baseline + 1,
        "{label}: unexpected organization event count"
    );
    assert_eq!(
        events[0]
            .split_whitespace()
            .find_map(|field| field.strip_prefix("refusal=")),
        Some(case.refusal),
        "{label}: wrong closed classification"
    );
    assert_safe_logs(&after.join("\n"), &[case.statement], &label);
    assert!(
        f.read("SELECT id FROM organization_identity")
            .await
            .rows
            .is_empty(),
        "{label}: organization identity written"
    );
    assert!(
        f.read("SELECT id FROM organization_change_records")
            .await
            .rows
            .is_empty(),
        "{label}: organization change written"
    );
}
#[tokio::test]
async fn apply_time_expiry_and_invalid_configuration_emit_one_safe_observation() {
    let f = Fixture::new().await;
    let runtime = f.runtime().await;
    let compact =
        signed::statement_with_times(ORG, NOW_SECONDS, NOW_SECONDS - 60, NOW_SECONDS + 1).unwrap();
    let _: bool = runtime
        .control("hold-central", &"beforeSignInSample")
        .await
        .unwrap();
    let body = json!({"organizationId":ORG,"organizationStatement":compact});
    let request = async { f.post(PROVISION, &body).await };
    let release = async {
        let _: bool = runtime
            .control("until-held-central", &"beforeSignInSample")
            .await
            .unwrap();
        f.clock.advance(std::time::Duration::from_secs(1)).unwrap();
        f.runtime().await;
        let _: bool = runtime
            .control("release-central", &"beforeSignInSample")
            .await
            .unwrap();
    };
    let (reply, ()) = tokio::join!(request, release);
    assert_eq!(reply, (422, serde_json::Value::Null));
    f.host.until_log("refusal=Expired").await;
    let logs = f.host.captured_logs().join("\n");
    assert_eq!(
        logs.lines()
            .filter(|line| line.contains("central organization:") && line.contains(ORG))
            .count(),
        1
    );
    assert_safe_logs(&logs, &[compact], "apply-time expiry");
    f.close().await;
    let f = Fixture::new_with_issuer("").await;
    let compact = signed::stamp(ORG, NOW_SECONDS).unwrap();
    assert_eq!(
        f.post(
            PROVISION,
            &json!({"organizationId":ORG,"organizationStatement":compact})
        )
        .await,
        (503, serde_json::Value::Null)
    );
    f.host.until_log("refusal=InvalidConfiguration").await;
    let logs = f.host.captured_logs().join("\n");
    assert_eq!(
        logs.lines()
            .filter(|line| line.contains("central organization:") && line.contains(ORG))
            .count(),
        1
    );
    assert_safe_logs(&logs, &[compact], "invalid configuration");
    assert_eq!(f.keys.fetches(), 0);
    f.close().await;
}
#[tokio::test]
async fn unreachable_real_jwks_source_emits_one_unavailable_classification_without_material() {
    let mut f = Fixture::new().await;
    assert_eq!(f.provision().await.0, 200);
    let replacement = signed::SignInKeys::start().await.unwrap();
    let old = std::mem::replace(&mut f.keys, replacement);
    old.shutdown().await.unwrap();
    let compact = signed::statement_with_header(
        ORG,
        NOW_SECONDS,
        json!({"typ":"fleet-organization-stamp+jwt","alg":"EdDSA","kid":"unreachable-key"}),
        9,
    )
    .unwrap();
    let before = f.counts().await;
    assert_eq!(
        f.post(
            PROVISION,
            &json!({"organizationId":ORG,"organizationStatement":compact})
        )
        .await,
        (503, serde_json::Value::Null)
    );
    f.host.until_log("refusal=VerificationUnavailable").await;
    let logs = f.host.captured_logs().join("\n");
    assert_eq!(
        logs.lines()
            .filter(|line| line.contains("central organization:") && line.contains(ORG))
            .count(),
        1
    );
    assert_safe_logs(&logs, &[compact], "unreachable JWKS source");
    assert_eq!(f.counts().await, before);
    f.close().await;
}
