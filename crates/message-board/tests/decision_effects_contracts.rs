use message_board::*;
use sha2::{Digest, Sha256};
mod support;

#[test]
fn fleet_effect_validation_refuses_a_post_even_when_its_placement_and_digest_match() {
    let placement = RecordPlacement::Fleet(FleetFeedEntry {
        position: FleetPosition::try_from(17).unwrap(),
        origin: support::actor_origin(),
    });
    for kind in [
        RecordKind::Workspace,
        RecordKind::Board,
        RecordKind::Adr,
        RecordKind::Project,
    ] {
        assert!(
            validate_fleet_decision_effect(&(
                RecordRef {
                    kind,
                    id: RecordId::generate()
                },
                placement.clone(),
            ))
            .is_ok()
        );
    }
    let post = (RecordRef::from(PostId::generate()), placement);
    let header = DecisionEffectsHeader::from_full(std::slice::from_ref(&post));
    assert!(header.matches_full(std::slice::from_ref(&post)));
    assert!(
        validate_fleet_decision_effect(&post).is_err(),
        "a matching receipt commitment cannot make a Post a Fleet record"
    );
    let mut workspace = (
        RecordRef::from(WorkspaceId::generate()),
        RecordPlacement::Unsequenced,
    );
    assert!(validate_fleet_decision_effect(&workspace).is_err());
    workspace.1 = support::record_envelope(RecordId::generate(), ProjectId::generate()).position;
    assert!(validate_fleet_decision_effect(&workspace).is_err());
}

#[test]
fn complete_page_decode_checks_its_digest_and_rejects_unknown_fields() {
    let full = project_effects().into_iter().take(2).collect::<Vec<_>>();
    let header = DecisionEffectsHeader::from_full(&full);
    let page = DecisionEffectsPage::from_full(full, &header, None).unwrap();
    let valid = serde_json::to_value(page).unwrap();
    for (field, value) in [
        ("digest", serde_json::json!("0".repeat(64))),
        ("extra", serde_json::json!(true)),
    ] {
        let mut bad = valid.clone();
        bad[field] = value;
        assert!(
            serde_json::from_value::<DecisionEffectsPage>(bad).is_err(),
            "{field}"
        );
    }
}

#[test]
fn legacy_inline_effect_arrays_are_refused_after_the_hard_cutover() {
    let old =
        serde_json::json!({"kind":"accepted","records":[],"positions":[],"change":"alreadySo"});
    assert!(serde_json::from_value::<RequestDecision>(old).is_err());
}

#[expect(
    clippy::unwrap_used,
    reason = "Invalid test fixtures must fail the test immediately."
)] // Fixture construction must fail on an invalid domain value.
fn project_effects() -> Vec<DecisionEffect> {
    let record: RecordRef = TaskId::generate().into();
    let fixture_envelope = support::record_envelope(record.id.clone(), ProjectId::generate());
    let mut maximum_origin = fixture_envelope.position.origin().unwrap().clone();
    maximum_origin.machine = MachineLabel::try_from("\"".repeat(256)).unwrap();
    (1..=600)
        .map(|position| {
            (
                record.clone(),
                RecordPlacement::Project(ProjectFeedEntry {
                    cursor: FeedCursor {
                        epoch: EpochNumber::INITIAL,
                        position: FeedPosition::try_from(position).unwrap(),
                    },
                    origin: maximum_origin.clone(),
                }),
            )
        })
        .collect()
}

#[test]
fn decision_pages_preserve_every_repeated_reference_and_the_full_digest() {
    let effects = project_effects();
    let header = DecisionEffectsHeader::from_full(&effects);
    let mut canonical = serde_json::to_value(&effects).unwrap();
    // ACP enables insertion-ordered JSON maps in combined package gates.
    // The reference stays lexical regardless of that dependency feature.
    canonical.sort_all_objects();
    let expected = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&canonical).unwrap())
    );
    assert_eq!(header.count, 600);
    assert_eq!(header.digest.as_str(), expected);
    let mut after = None;
    let mut rebuilt = Vec::new();
    let mut page_lengths = Vec::new();
    loop {
        let page =
            DecisionEffectsPage::from_full(effects.clone(), &header, after.as_ref()).unwrap();
        assert_eq!(page.total_count(), 600);
        assert_eq!(page.digest(), &header.digest);
        let wire = serde_json::to_vec(&page).unwrap();
        assert!(wire.len() < 1_048_576);
        assert_eq!(
            serde_json::from_slice::<DecisionEffectsPage>(&wire).unwrap(),
            page
        );
        page_lengths.push(page.entries().len());
        rebuilt.extend_from_slice(page.entries());
        after = page.next().cloned();
        if after.is_none() {
            break;
        }
    }
    assert_eq!(page_lengths, [256, 256, 88]);
    assert_eq!(rebuilt, effects);
}

#[test]
fn decision_status_and_empty_effect_change_are_independent() {
    for change in [ChangeOutcome::Changed, ChangeOutcome::AlreadySo] {
        let decision = StoredDecision::accepted(Vec::new(), change);
        assert!(
            matches!(decision.into_reply(Vec::new(), None).unwrap(), RequestDecision::Accepted { effects, change: actual } if actual == change && effects.entries().is_empty() && effects.next().is_none())
        );
    }
    let rejected = StoredDecision::Rejected {
        reasons: vec![Refusal::Archived],
    };
    assert_eq!(
        rejected.into_reply(project_effects(), None).unwrap(),
        RequestDecision::Rejected {
            reasons: vec![Refusal::Archived]
        }
    );
}

#[test]
fn decision_page_refuses_wrong_membership_cursor_and_digest() {
    let effects = project_effects();
    let mut header = DecisionEffectsHeader::from_full(&effects);
    for cursor in [
        RequestEffectsCursor::Fleet { ordinal: 0 },
        RequestEffectsCursor::Project {
            position: FeedPosition::ZERO,
        },
    ] {
        assert!(DecisionEffectsPage::from_full(effects.clone(), &header, Some(&cursor)).is_err());
    }
    header.digest = CanonicalHash::try_from("0".repeat(64)).unwrap();
    assert!(DecisionEffectsPage::from_full(effects, &header, None).is_err());
}
