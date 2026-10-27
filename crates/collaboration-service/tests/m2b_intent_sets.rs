#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    clippy::match_wildcard_for_single_variants,
    reason = "lint pass follows the crate split"
)]
//! NI-2 actor-by-actor re-resolution through real native claim transactions.
#[path = "support/board_fixture.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
#[expect(
    clippy::indexing_slicing,
    reason = "This test consumes a subset of the shared fixture and retains its fixed JSON construction."
)]
pub mod board_fixture;
#[path = "support/m2b_intents.rs"]
mod m2b_intents;
use board_fixture::TestError;
use m2b_intents::*;
use message_board::*;
use serde_json::json;
use std::collections::BTreeSet;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; Result propagates fixture errors."
)]
async fn role_addressing_emits_exactly_zero_one_or_two_holders() -> Result<(), TestError> {
    for role in ["reviewer", "advisor", "operator"] {
        for holders in 0..=2 {
            let fixture = IntentFixture::start().await?;
            let actors = [session("a")?, session("b")?];
            for actor in actors.iter().take(holders) {
                fixture.grant(role, actor).await?;
            }
            let cause = fixture.cause(vec![fixture.role_target(role)]).await?;
            let intents = fixture.intents(&cause).await?;
            if holders == 0 {
                assert_eq!(intents.len(), 1);
                assert!(intents.iter().all(|intent| matches!(
                    intent.recipient(),
                    IntentRecipient::NoHolder { .. }
                ) && matches!(
                    intent.state(),
                    IntentState::Refused {
                        reason: IntentRefusal::NoHolder {}
                    }
                )));
            } else {
                assert_eq!(intents.len(), holders);
                let actual: BTreeSet<_> = intents
                    .iter()
                    .filter_map(|intent| match intent.recipient() {
                        IntentRecipient::Resolved(recipient) => recipient.actor.declared_identity(),
                        IntentRecipient::NoHolder { .. } => None,
                    })
                    .collect();
                assert_eq!(actual, actors.iter().take(holders).cloned().collect());
            }
            fixture.board.stop().await?;
        }
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; Result propagates fixture errors."
)]
async fn retained_holder_keeps_identity_updates_revision_and_adds_new_holder()
-> Result<(), TestError> {
    let fixture = IntentFixture::start().await?;
    let first = session("first")?;
    let second = session("second")?;
    fixture.grant("reviewer", &first).await?;
    let cause = fixture.cause(vec![fixture.role_target("reviewer")]).await?;
    let before = fixture.intents(&cause).await?;
    let original = actor_intent(&before, &first)?.intent_id.clone();
    fixture.grant("reviewer", &second).await?;
    let result = fixture.claim(&original).await?;
    assert_eq!(result.get("kind"), Some(&json!("claimed")), "{result}");
    let after = fixture.intents(&cause).await?;
    assert_eq!(after.len(), 2);
    let retained = actor_intent(&after, &first)?;
    assert_eq!(retained.intent_id, original);
    assert!(matches!(retained.state(), IntentState::HandedOff { .. }));
    let revision = fixture.revision().await?;
    assert!(
        matches!(retained.recipient(), IntentRecipient::Resolved(recipient) if recipient.via_role.as_ref().is_some_and(|role| role.assignment_revision == revision))
    );
    let added = actor_intent(&after, &second)?;
    assert!(matches!(added.state(), IntentState::Emitted {}));
    assert_eq!(
        fixture.claim(&added.intent_id).await?.get("kind"),
        Some(&json!("claimed"))
    );
    fixture.board.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; Result propagates fixture errors."
)]
async fn shrinking_set_supersedes_only_removed_actor_and_keeps_retained_identity()
-> Result<(), TestError> {
    let fixture = IntentFixture::start().await?;
    let first = session("first")?;
    let second = session("second")?;
    fixture.grant("reviewer", &first).await?;
    fixture.grant("reviewer", &second).await?;
    let cause = fixture.cause(vec![fixture.role_target("reviewer")]).await?;
    let before = fixture.intents(&cause).await?;
    let removed = actor_intent(&before, &first)?.intent_id.clone();
    let retained = actor_intent(&before, &second)?.intent_id.clone();
    fixture.release("reviewer", &first).await?;
    let result = fixture.claim(&removed).await?;
    assert_eq!(result, json!({"kind":"superseded","replacements":[]}));
    assert_eq!(
        fixture.claim(&retained).await?.get("kind"),
        Some(&json!("claimed"))
    );
    let after = fixture.intents(&cause).await?;
    assert_eq!(after.len(), 2);
    assert!(matches!(
        actor_intent(&after, &first)?.state(),
        IntentState::Superseded {}
    ));
    assert_eq!(actor_intent(&after, &second)?.intent_id, retained);
    fixture.board.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; Result propagates fixture errors."
)]
async fn returning_actor_reuses_original_intent_before_handoff() -> Result<(), TestError> {
    let fixture = IntentFixture::start().await?;
    let first = session("first")?;
    let second = session("second")?;
    fixture.grant("implementer", &first).await?;
    let cause = fixture
        .cause(vec![fixture.role_target("implementer")])
        .await?;
    let original = actor_intent(&fixture.intents(&cause).await?, &first)?
        .intent_id
        .clone();
    fixture.grant("implementer", &second).await?;
    let first_claim = fixture.claim(&original).await?;
    assert_eq!(first_claim.get("kind"), Some(&json!("superseded")));
    let second_id = actor_intent(&fixture.intents(&cause).await?, &second)?
        .intent_id
        .clone();
    fixture.grant("implementer", &first).await?;
    assert_eq!(
        fixture.claim(&second_id).await?,
        json!({"kind":"superseded","replacements":[original]})
    );
    let after = fixture.intents(&cause).await?;
    assert_eq!(
        after.len(),
        2,
        "returning actor must not get a fresh identity"
    );
    assert!(matches!(
        actor_intent(&after, &first)?.state(),
        IntentState::Emitted {}
    ));
    assert_eq!(
        fixture.claim(&original).await?.get("kind"),
        Some(&json!("claimed"))
    );
    assert!(fixture.claim(&original).await?.is_null());
    fixture.board.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; Result propagates fixture errors."
)]
async fn empty_role_is_terminal_for_this_cause_even_when_holder_returns() -> Result<(), TestError> {
    let fixture = IntentFixture::start().await?;
    let first = session("first")?;
    fixture.grant("reviewer", &first).await?;
    let cause = fixture.cause(vec![fixture.role_target("reviewer")]).await?;
    let original = actor_intent(&fixture.intents(&cause).await?, &first)?
        .intent_id
        .clone();
    fixture.release("reviewer", &first).await?;
    assert_eq!(
        fixture.claim(&original).await?,
        json!({"kind":"superseded","replacements":[]})
    );
    fixture.grant("reviewer", &first).await?;
    assert!(fixture.claim(&original).await?.is_null());
    let after = fixture.intents(&cause).await?;
    assert_eq!(after.len(), 2);
    assert!(
        after
            .iter()
            .any(|intent| matches!(intent.recipient(), IntentRecipient::NoHolder { .. }))
    );
    assert!(matches!(
        actor_intent(&after, &first)?.state(),
        IntentState::Superseded {}
    ));
    fixture.board.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; Result propagates fixture errors."
)]
async fn named_target_wins_role_dedup_and_survives_role_removal() -> Result<(), TestError> {
    let fixture = IntentFixture::start().await?;
    let first = session("first")?;
    fixture.grant("reviewer", &first).await?;
    // Role deliberately comes first in input: precedence is semantic, not input order.
    let cause = fixture
        .cause(vec![
            fixture.role_target("reviewer"),
            json!({"kind":"actor","recipient":first}),
        ])
        .await?;
    let before = fixture.intents(&cause).await?;
    assert_eq!(before.len(), 1);
    let original = actor_intent(&before, &first)?;
    assert!(
        matches!(original.recipient(), IntentRecipient::Resolved(recipient) if recipient.via_role.is_none())
    );
    fixture.release("reviewer", &first).await?;
    assert_eq!(
        fixture.claim(&original.intent_id).await?.get("kind"),
        Some(&json!("claimed"))
    );
    fixture.board.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; Result propagates fixture errors."
)]
async fn handed_off_recipient_stays_frozen_and_a_return_adds_no_second_handoff()
-> Result<(), TestError> {
    let fixture = IntentFixture::start().await?;
    let first = session("first")?;
    let second = session("second")?;
    fixture.grant("implementer", &first).await?;
    let cause = fixture
        .cause(vec![fixture.role_target("implementer")])
        .await?;
    let original = actor_intent(&fixture.intents(&cause).await?, &first)?.clone();
    assert_eq!(
        fixture.claim(&original.intent_id).await?.get("kind"),
        Some(&json!("claimed"))
    );
    fixture.grant("implementer", &second).await?;
    assert!(fixture.claim(&original.intent_id).await?.is_null());
    fixture.grant("implementer", &first).await?;
    assert!(fixture.claim(&original.intent_id).await?.is_null());
    let after = fixture.intents(&cause).await?;
    assert_eq!(after.len(), 1);
    assert_eq!(
        actor_intent(&after, &first)?.recipient(),
        original.recipient()
    );
    assert!(matches!(
        actor_intent(&after, &first)?.state(),
        IntentState::HandedOff { .. }
    ));
    fixture.board.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; Result propagates fixture errors."
)]
async fn deferred_human_commits_re_resolution_and_returns_the_added_local_intent()
-> Result<(), TestError> {
    let fixture = IntentFixture::start().await?;
    let human = Identity::Human {
        human_id: HumanId::try_from("human-reviewer".to_owned())?,
    };
    let local = session("local-reviewer")?;
    fixture.grant("reviewer", &human).await?;
    let cause = fixture.cause(vec![fixture.role_target("reviewer")]).await?;
    let original = actor_intent(&fixture.intents(&cause).await?, &human)?
        .intent_id
        .clone();
    fixture.grant("reviewer", &local).await?;
    let claim = fixture.claim(&original).await?;
    // Read persistence before reply-shape assertions: a rollback must fail this case.
    let after = fixture.intents(&cause).await?;
    assert_eq!(
        after.len(),
        2,
        "deferred re-resolution lost the local co-holder: {claim}"
    );
    let kept = actor_intent(&after, &human)?;
    assert_eq!(kept.intent_id, original);
    assert!(matches!(kept.state(), IntentState::Emitted {}));
    let revision = fixture.revision().await?;
    assert!(
        matches!(kept.recipient(), IntentRecipient::Resolved(recipient) if recipient.via_role.as_ref().is_some_and(|role| role.assignment_revision == revision))
    );
    let added = actor_intent(&after, &local)?;
    assert_eq!(claim.get("kind"), Some(&json!("deferred")), "{claim}");
    assert_eq!(claim.get("replacements"), Some(&json!([added.intent_id])));
    assert_eq!(
        fixture.claim(&added.intent_id).await?.get("kind"),
        Some(&json!("claimed"))
    );
    fixture.board.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; Result propagates fixture errors."
)]
async fn retained_claim_reinstates_removed_co_holder_under_its_original_identity()
-> Result<(), TestError> {
    let fixture = IntentFixture::start().await?;
    let first = session("retained-a")?;
    let second = session("returning-b")?;
    fixture.grant("reviewer", &first).await?;
    fixture.grant("reviewer", &second).await?;
    let cause = fixture.cause(vec![fixture.role_target("reviewer")]).await?;
    let before = fixture.intents(&cause).await?;
    let first_id = actor_intent(&before, &first)?.intent_id.clone();
    let second_id = actor_intent(&before, &second)?.intent_id.clone();
    fixture.release("reviewer", &second).await?;
    let removed = fixture.claim(&second_id).await?;
    assert_eq!(removed.get("kind"), Some(&json!("superseded")), "{removed}");
    fixture.grant("reviewer", &second).await?;
    let retained = fixture.claim(&first_id).await?;
    let after = fixture.intents(&cause).await?;
    assert_eq!(after.len(), 2, "returning actor got a duplicate ID");
    let reinstated = actor_intent(&after, &second)?;
    assert_eq!(reinstated.intent_id, second_id);
    assert!(matches!(reinstated.state(), IntentState::Emitted {}));
    assert_eq!(retained.get("kind"), Some(&json!("claimed")), "{retained}");
    assert_eq!(retained.get("replacements"), Some(&json!([second_id])));
    assert_eq!(
        fixture.claim(&second_id).await?.get("kind"),
        Some(&json!("claimed"))
    );
    assert!(fixture.claim(&second_id).await?.is_null());
    fixture.board.stop().await
}
