use message_board::{public_replies as public, *};
use serde_json::{Value, json};
mod support;

#[allow(clippy::unwrap_used)] // Fixture construction must fail on invalid domain values.
fn effects(count: u64, fleet: bool) -> Vec<DecisionEffect> {
    let reference: RecordRef = if fleet {
        WorkspaceId::generate().into()
    } else {
        TaskId::generate().into()
    };
    let fixture = support::record_envelope(reference.id.clone(), ProjectId::generate());
    let origin = fixture.position.origin().unwrap().clone();
    (1..=count)
        .map(|position| {
            let placement = if fleet {
                RecordPlacement::Fleet(FleetFeedEntry {
                    position: FleetPosition::try_from(position).unwrap(),
                    origin: origin.clone(),
                })
            } else {
                RecordPlacement::Project(ProjectFeedEntry {
                    cursor: FeedCursor {
                        epoch: EpochNumber::INITIAL,
                        position: FeedPosition::try_from(position).unwrap(),
                    },
                    origin: origin.clone(),
                })
            };
            (reference.clone(), placement)
        })
        .collect()
}

fn prefix_wire(fleet: bool) -> Value {
    let full = effects(300, fleet);
    let header = DecisionEffectsHeader::from_full(&full);
    json!({
        "entries":full.into_iter().take(256).collect::<Vec<_>>(),
        "totalCount":header.count,"digest":header.digest,
        "next":"opaque-router-continuation"
    })
}

#[test]
fn public_pages_use_opaque_continuations_while_native_pages_keep_raw_keysets() {
    for fleet in [false, true] {
        let expected = prefix_wire(fleet);
        let page: public::DecisionEffectsPage = serde_json::from_value(expected.clone()).unwrap();
        assert_eq!(page.entries().len(), 256);
        assert_eq!(page.total_count(), 300);
        assert_eq!(page.next(), Some("opaque-router-continuation"));
        assert_eq!(serde_json::to_value(page).unwrap(), expected);

        let full = effects(300, fleet);
        let header = DecisionEffectsHeader::from_full(&full);
        let raw = DecisionEffectsPage::from_full(full, &header, None).unwrap();
        assert!(public::DecisionEffectsPage::from_native(raw.clone(), None).is_err());
        let projected = public::DecisionEffectsPage::from_native(
            raw.clone(),
            Some("opaque-router-continuation".to_owned()),
        )
        .unwrap();
        assert_eq!(projected.entries(), raw.entries());
        assert_eq!(projected.digest(), raw.digest());
        let raw = serde_json::to_value(raw).unwrap();
        assert!(raw.get("next").is_some_and(Value::is_object));
        assert!(serde_json::from_value::<public::DecisionEffectsPage>(raw).is_err());
    }
}

#[test]
fn public_decode_keeps_page_bounds_and_placement_validation() {
    let base = prefix_wire(false);
    let mut invalid = Vec::new();
    let mut oversized = base.clone();
    oversized["entries"] = serde_json::to_value(effects(257, false)).unwrap();
    invalid.push(("oversized page", oversized));
    let mut short = base.clone();
    short["entries"].as_array_mut().unwrap().pop();
    invalid.push(("short continuing page", short));
    let mut wrong_count = base.clone();
    wrong_count["totalCount"] = json!(255);
    invalid.push(("count below page", wrong_count));
    let mut unordered = base.clone();
    unordered["entries"].as_array_mut().unwrap().swap(0, 1);
    invalid.push(("unordered project positions", unordered));
    let mut unsequenced = base.clone();
    let first = unsequenced["entries"]
        .as_array_mut()
        .unwrap()
        .first_mut()
        .unwrap();
    first.as_array_mut().unwrap().pop();
    first
        .as_array_mut()
        .unwrap()
        .push(json!({"kind":"unsequenced"}));
    invalid.push(("unsequenced placement", unsequenced));
    let mut empty_cursor = base.clone();
    empty_cursor["next"] = json!("");
    invalid.push(("empty continuation", empty_cursor));
    let mut unknown = base;
    unknown["extra"] = json!(true);
    invalid.push(("unknown field", unknown));
    for (reason, wire) in invalid {
        assert!(
            serde_json::from_value::<public::DecisionEffectsPage>(wire).is_err(),
            "public decoder admitted {reason}"
        );
    }
}

#[test]
fn public_complete_page_keeps_the_canonical_digest_check() {
    let full = effects(2, false);
    let header = DecisionEffectsHeader::from_full(&full);
    let expected = json!({"entries":full,"totalCount":2,"digest":header.digest,"next":null});
    let valid: public::DecisionEffectsPage = serde_json::from_value(expected.clone()).unwrap();
    assert_eq!(serde_json::to_value(valid).unwrap(), expected);
    let mut invalid = expected;
    invalid["digest"] = json!("0".repeat(64));
    assert!(serde_json::from_value::<public::DecisionEffectsPage>(invalid).is_err());
}

#[test]
fn public_receipt_read_requires_an_explicit_scope_and_an_opaque_after_value() {
    let expected = json!({
        "scope":{"kind":"project","projectId":ProjectId::generate()},
        "request":RequestId::generate(),"after":"opaque-router-continuation"
    });
    let query: public::RequestEffectsRequest = serde_json::from_value(expected.clone()).unwrap();
    assert_eq!(serde_json::to_value(query).unwrap(), expected);
    let mut raw = expected;
    raw["after"] = json!({"kind":"project","position":256});
    assert!(serde_json::from_value::<public::RequestEffectsRequest>(raw).is_err());
}
