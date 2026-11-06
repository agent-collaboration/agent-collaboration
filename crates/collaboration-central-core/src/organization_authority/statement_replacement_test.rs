use super::{
    OrganizationStatementReplacement as Decision,
    OrganizationStatementReplacementRefusal as Refusal, decide_organization_statement_replacement,
};
use crate::organization_authority::{
    VerifiedFleetOrganizationStatement,
    fleet_statement_test_support::{claims, verified_claims, verified_fixture},
};
use serde_json::json;

async fn replacement_for(
    stored: &VerifiedFleetOrganizationStatement,
    issued_at: i64,
) -> VerifiedFleetOrganizationStatement {
    let mut input = claims();
    input["iat"] = json!(issued_at);
    input["replaces"] = json!(stored.digest().as_str());
    verified_claims(input).await
}

#[tokio::test(start_paused = true)]
async fn fleet_replacement_initializes_only_without_a_signed_predecessor() {
    let first = verified_fixture().await;
    let replacement = replacement_for(&first, 1_799_999_950).await;

    assert_eq!(
        decide_organization_statement_replacement(None, &first),
        Ok(Decision::Initialize)
    );
    assert_eq!(
        decide_organization_statement_replacement(None, &replacement),
        Err(Refusal::StaleOrganizationStatement)
    );
}

#[tokio::test(start_paused = true)]
async fn fleet_replacement_accepts_the_matching_signed_predecessor() {
    let first = verified_fixture().await;
    let replacement = replacement_for(&first, 1_799_999_950).await;

    let decision = decide_organization_statement_replacement(Some(&first), &replacement);

    assert_eq!(decision, Ok(Decision::Replace));
}

#[tokio::test(start_paused = true)]
async fn fleet_replacement_keeps_a_setup_retry_after_a_lost_reply() {
    let first = verified_fixture().await;
    let mut retry_claims = claims();
    retry_claims["iat"] = json!(1_799_999_951);
    let retry = verified_claims(retry_claims).await;

    let decision = decide_organization_statement_replacement(Some(&first), &retry);

    assert_eq!(decision, Ok(Decision::AlreadyProvisioned));
    assert_ne!(first.digest(), retry.digest());
}

#[tokio::test(start_paused = true)]
async fn fleet_replacement_clock_correction_cannot_rebase_a_delayed_candidate() {
    let first = verified_fixture().await;
    let clock_ahead = replacement_for(&first, 1_799_999_980).await;
    let clock_corrected = replacement_for(&first, 1_799_999_950).await;
    assert!(clock_ahead.issued_at() > clock_corrected.issued_at());

    let first_decision = decide_organization_statement_replacement(Some(&first), &clock_corrected);
    let delayed_decision =
        decide_organization_statement_replacement(Some(&clock_corrected), &clock_ahead);

    assert_eq!(first_decision, Ok(Decision::Replace));
    assert_eq!(delayed_decision, Err(Refusal::StaleOrganizationStatement));
}

#[tokio::test(start_paused = true)]
async fn fleet_replacement_two_candidates_for_one_predecessor_have_one_decision_winner() {
    let first = verified_fixture().await;
    let winner = replacement_for(&first, 1_799_999_950).await;
    let other = replacement_for(&first, 1_799_999_951).await;

    let admitted = decide_organization_statement_replacement(Some(&first), &winner);
    // Model the next serialized transaction reading the already committed winner.
    let next = decide_organization_statement_replacement(Some(&winner), &other);

    assert_eq!(admitted, Ok(Decision::Replace));
    assert_eq!(next, Err(Refusal::StaleOrganizationStatement));
    // This is pure current-state policy; it does not claim real concurrent SQL serialization.
}

#[tokio::test(start_paused = true)]
async fn fleet_replacement_original_setup_replay_preserves_the_replacement() {
    let original = verified_fixture().await;
    let stored = replacement_for(&original, 1_799_999_950).await;
    let before_digest = stored.digest().clone();
    let before_key = stored.key_id().to_owned();

    let decision = decide_organization_statement_replacement(Some(&stored), &original);

    assert_eq!(decision, Ok(Decision::AlreadyProvisioned));
    assert_eq!(stored.digest(), &before_digest);
    assert_eq!(stored.key_id(), before_key);
    // The decision cannot mutate a record count: no storage or apply operation exists here.
}
