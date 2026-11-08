use super::{
    CurrentStamp, OrganizationStatementReplacement as Decision,
    OrganizationStatementReplacementRefusal as Refusal, decide_organization_statement_replacement,
};
use crate::organization_authority::{
    VerifiedFleetOrganizationStatement,
    fleet_statement_test_support::{
        claims, verified_claims, verified_fixture, verified_foreign_organization, verified_next_key,
    },
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

    let decision = decide_organization_statement_replacement(
        Some(&CurrentStamp::of_verified(&first)),
        &replacement,
    );

    assert_eq!(decision, Ok(Decision::Replace));
}

#[tokio::test(start_paused = true)]
async fn fleet_replacement_keeps_a_setup_retry_after_a_lost_reply() {
    let first = verified_fixture().await;
    let mut retry_claims = claims();
    retry_claims["iat"] = json!(1_799_999_951);
    let retry = verified_claims(retry_claims).await;

    let decision =
        decide_organization_statement_replacement(Some(&CurrentStamp::of_verified(&first)), &retry);

    assert_eq!(decision, Ok(Decision::AlreadyProvisioned));
    assert_ne!(first.digest(), retry.digest());
}

#[tokio::test(start_paused = true)]
async fn fleet_replacement_clock_correction_cannot_rebase_a_delayed_candidate() {
    let first = verified_fixture().await;
    let clock_ahead = replacement_for(&first, 1_799_999_980).await;
    let clock_corrected = replacement_for(&first, 1_799_999_950).await;
    assert!(clock_ahead.issued_at() > clock_corrected.issued_at());

    let first_decision = decide_organization_statement_replacement(
        Some(&CurrentStamp::of_verified(&first)),
        &clock_corrected,
    );
    let delayed_decision = decide_organization_statement_replacement(
        Some(&CurrentStamp::of_verified(&clock_corrected)),
        &clock_ahead,
    );

    assert_eq!(first_decision, Ok(Decision::Replace));
    assert_eq!(delayed_decision, Err(Refusal::StaleOrganizationStatement));
}

#[tokio::test(start_paused = true)]
async fn fleet_replacement_two_candidates_for_one_predecessor_have_one_decision_winner() {
    let first = verified_fixture().await;
    let winner = replacement_for(&first, 1_799_999_950).await;
    let other = replacement_for(&first, 1_799_999_951).await;

    let admitted = decide_organization_statement_replacement(
        Some(&CurrentStamp::of_verified(&first)),
        &winner,
    );
    // Model the next serialized transaction reading the already committed winner.
    let next = decide_organization_statement_replacement(
        Some(&CurrentStamp::of_verified(&winner)),
        &other,
    );

    assert_eq!(admitted, Ok(Decision::Replace));
    assert_eq!(next, Err(Refusal::StaleOrganizationStatement));
    // This is pure current-state policy; it does not claim real concurrent SQL serialization.
}

#[tokio::test(start_paused = true)]
async fn fleet_replacement_original_setup_replay_preserves_the_replacement() {
    let original = verified_fixture().await;
    let mut input = claims();
    input["replaces"] = json!(original.digest().as_str());
    let stored = verified_next_key(input).await;
    assert_ne!(original.key_id(), stored.key_id());
    let before_digest = stored.digest().clone();
    let before_key = stored.key_id().to_owned();

    let decision = decide_organization_statement_replacement(
        Some(&CurrentStamp::of_verified(&stored)),
        &original,
    );

    assert_eq!(decision, Ok(Decision::AlreadyProvisioned));
    assert_eq!(stored.digest(), &before_digest);
    assert_eq!(stored.key_id(), before_key);
    // The decision cannot mutate a record count: no storage or apply operation exists here.
}

#[tokio::test(start_paused = true)]
async fn fleet_replacement_never_changes_the_stored_organization() {
    let stored = verified_fixture().await;
    let foreign = verified_foreign_organization().await;

    let decision = decide_organization_statement_replacement(
        Some(&CurrentStamp::of_verified(&stored)),
        &foreign,
    );

    assert_eq!(decision, Err(Refusal::OrganizationMismatch));
}

#[tokio::test(start_paused = true)]
async fn fleet_replacement_byte_identical_replay_of_a_renewal_answers_without_a_swap() {
    // Wrong build caught: a lost-reply retry of a committed renewal is refused as
    // stale (its replaces names the predecessor, not the stored statement itself).
    let first = verified_fixture().await;
    let renewal = replacement_for(&first, 1_799_999_950).await;

    let replay = decide_organization_statement_replacement(
        Some(&CurrentStamp::of_verified(&renewal)),
        &renewal,
    );

    assert_eq!(replay, Ok(Decision::AlreadyProvisioned));
}
