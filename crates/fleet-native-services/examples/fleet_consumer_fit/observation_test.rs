use super::*;

#[test]
fn no_observation_cannot_claim_a_complete_journey() {
    assert_eq!(Observations::default().exit_code(), 3);
}

#[test]
fn successful_controls_do_not_erase_separate_coverage_gaps() {
    let mut observations = Observations::default();
    observations.check("actual-answer", true, json!("answer"), json!("answer"));
    observations.gap("separate-runtime", "not observed by this native journey");
    assert_eq!(observations.exit_code(), 3);
}

#[test]
fn a_mismatch_remains_failure_beside_passing_and_unavailable_controls() {
    let mut observations = Observations::default();
    observations.check("healthy-control", true, json!(1), json!(1));
    observations.check("wrong-answer", false, json!("expected"), json!("wrong"));
    observations.gap("other-runtime", "unavailable");
    assert_eq!(observations.exit_code(), 1);
}

#[test]
fn observed_controls_without_failures_or_gaps_complete() {
    let mut observations = Observations::default();
    observations.check(
        "actual-answer",
        true,
        json!({"value": 2}),
        json!({"value": 2}),
    );
    assert_eq!(observations.exit_code(), 0);
}
