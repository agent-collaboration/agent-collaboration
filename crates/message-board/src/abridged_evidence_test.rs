use super::{AbridgedEvidence, EvidenceValidationError};
use crate::{CanonicalHash, TaskId};

fn task() -> TaskId {
    "019f0000-0000-7000-8000-00000000ab01"
        .to_owned()
        .try_into()
        .unwrap()
}
fn full_digest() -> CanonicalHash {
    // Independent SHA-256 of the literal canonical JSON array containing task().
    "363157e0c22f58267430874d4c97cbb10b1110f7210e568ad3b135db559723cb"
        .to_owned()
        .try_into()
        .unwrap()
}

#[test]
fn complete_evidence_parts_keep_the_literal_commitment_and_match_serde_validation() {
    let evidence = AbridgedEvidence::from_parts(vec![task()], 1, full_digest(), false).unwrap();
    assert_eq!(evidence.entries(), [task()]);
    assert_eq!(evidence.total_count(), 1);
    assert_eq!(evidence.digest(), &full_digest());
    assert!(!evidence.is_abridged());
    assert_eq!(
        serde_json::from_value::<AbridgedEvidence<TaskId>>(
            serde_json::to_value(&evidence).unwrap()
        )
        .unwrap(),
        evidence
    );
    let bad: CanonicalHash = "00".repeat(32).try_into().unwrap();
    assert_eq!(
        AbridgedEvidence::from_parts(vec![task()], 1, bad.clone(), false),
        Err(EvidenceValidationError::Digest)
    );
    let wire = serde_json::json!({"entries":[task()],"totalCount":1,"digest":bad,"abridged":false});
    assert!(serde_json::from_value::<AbridgedEvidence<TaskId>>(wire).is_err());
}

#[test]
fn abridged_parts_preserve_the_unknown_full_digest_and_maximum_total() {
    let digest: CanonicalHash = "ab".repeat(32).try_into().unwrap();
    let entries = vec![task(); 256];
    let evidence =
        AbridgedEvidence::from_parts(entries.clone(), u64::MAX, digest.clone(), true).unwrap();
    assert_eq!(evidence.entries(), entries);
    assert_eq!(evidence.total_count(), u64::MAX);
    assert_eq!(evidence.digest(), &digest);
    assert!(evidence.is_abridged());
    assert_eq!(
        serde_json::from_value::<AbridgedEvidence<TaskId>>(
            serde_json::to_value(&evidence).unwrap()
        )
        .unwrap(),
        evidence
    );
}

#[test]
fn evidence_parts_and_serde_refuse_the_same_inconsistent_counts_and_flags() {
    for (returned, total, abridged) in [
        (1, 0, false),
        (1, 2, false),
        (1, 1, true),
        (255, 257, true),
        (256, 257, false),
        (257, 257, true),
    ] {
        let entries = vec![task(); returned];
        assert_eq!(
            AbridgedEvidence::from_parts(entries.clone(), total, full_digest(), abridged),
            Err(EvidenceValidationError::Count)
        );
        let wire = serde_json::json!({"entries":entries,"totalCount":total,"digest":full_digest(),"abridged":abridged});
        assert!(serde_json::from_value::<AbridgedEvidence<TaskId>>(wire).is_err());
    }
}
