use message_board::*;
use sha2::{Digest, Sha256};

#[test]
fn ambiguous_subject_evidence_is_a_bounded_non_exhaustive_prefix() {
    // The rule must inspect every eligible assignment before shortening its evidence.
    let assignments: Vec<_> = (0..300)
        .map(|_| Assignment::Implementer(WorkstreamId::generate()))
        .collect();
    let refusal = work_rules::resolve_trail_subject(None, &assignments).unwrap_err();
    let wire = serde_json::to_value(refusal).unwrap();
    let evidence = &wire["assignments"];
    assert_eq!(evidence["entries"].as_array().map(Vec::len), Some(256));
    assert_eq!(evidence["totalCount"], 300);
    assert_eq!(evidence["abridged"], true);
    assert_eq!(
        evidence["entries"],
        serde_json::to_value(&assignments[..256]).unwrap()
    );
    let full_digest = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&assignments).unwrap())
    );
    assert_eq!(evidence["digest"], full_digest);
    let decoded: TrailRefusal = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), wire);
}

#[test]
fn evidence_digest_matches_the_canonical_full_list_fixture() {
    let entries = vec![
        Assignment::Implementer(
            WorkstreamId::try_from("01900000-0000-7000-8000-000000000001".to_owned()).unwrap(),
        ),
        Assignment::Verifier(
            WorkstreamId::try_from("01900000-0000-7000-8000-000000000002".to_owned()).unwrap(),
        ),
    ];
    let evidence = AbridgedEvidence::from_full(entries);
    // Independently encoded canonical JSON and SHA-256, not the production helper.
    assert_eq!(
        evidence.digest().as_str(),
        "9ad17d5358a2327ba790b9338ea427a11bbb06952cc1e0833ad999a0597e43cd"
    );
    assert!(!evidence.is_abridged());
    assert_eq!(evidence.total_count(), 2);
}

#[test]
fn authoritative_evidence_decode_rejects_inconsistent_or_open_shapes() {
    let entries = vec![TaskId::generate(); 257];
    let full = serde_json::to_value(AbridgedEvidence::from_full(entries[..2].to_vec())).unwrap();
    let abridged = serde_json::to_value(AbridgedEvidence::from_full(entries.clone())).unwrap();
    assert!(serde_json::from_value::<AbridgedEvidence<TaskId>>(abridged.clone()).is_ok());
    for (key, value) in [
        ("totalCount", serde_json::json!(3)),
        ("abridged", serde_json::json!(true)),
        ("digest", serde_json::json!("0".repeat(64))),
        ("unexpected", serde_json::json!(true)),
    ] {
        let mut corrupt = full.clone();
        corrupt[key] = value;
        assert!(
            serde_json::from_value::<AbridgedEvidence<TaskId>>(corrupt).is_err(),
            "{key}"
        );
    }
    let mut too_many = abridged;
    too_many["entries"] = serde_json::to_value(entries).unwrap();
    assert!(serde_json::from_value::<AbridgedEvidence<TaskId>>(too_many).is_err());
    assert!(serde_json::from_value::<AbridgedEvidence<TaskId>>(serde_json::json!([])).is_err());
}
