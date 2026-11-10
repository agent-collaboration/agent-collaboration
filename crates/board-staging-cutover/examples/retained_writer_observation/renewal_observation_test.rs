use super::*;
type TestResult = Result<(), Box<dyn std::error::Error>>;
#[test]
fn only_matching_fresh_public_renewal_qualifies_not_original_stale_or_refused_authority()
-> TestResult {
    let database: DatabaseId =
        serde_json::from_value(serde_json::json!("01900000-0000-7000-8000-000000000001"))?;
    let holder: PeerId =
        serde_json::from_value(serde_json::json!("01900000-0000-7000-8000-000000000002"))?;
    let expected = Expected {
        database,
        holder,
        epoch: Epoch::new(2)?,
        window_start: 1000,
        window_end: 2000,
    };
    let raw = serde_json::json!({"kind":"renewal","requestStarted":1500,"answer":{"kind":"renewed","value":{"recorded":{"epoch":2,"expiresAt":5000},"current":{"database":database,"holder":holder,"epoch":2,"state":"held","expiresAt":5000,"remainingMs":3500},"transferInProgress":null,"replayed":false}}});
    assert_eq!(
        verify(&raw.to_string(), &expected)?["currentRenewalObserved"],
        true
    );
    let mut stale = raw.clone();
    stale["requestStarted"] = serde_json::json!(999);
    assert!(verify(&stale.to_string(), &expected).is_err());
    let mut wrong = raw.clone();
    wrong["answer"]["value"]["current"]["epoch"] = serde_json::json!(3);
    assert!(verify(&wrong.to_string(), &expected).is_err());
    let mut refused = raw;
    refused["answer"] = serde_json::json!({"kind":"refused","value":{"kind":"notRegistered"}});
    assert!(verify(&refused.to_string(), &expected).is_err());
    assert!(verify("null", &expected).is_err());
    Ok(())
}
