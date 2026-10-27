//! The library identity keeps the original address bytes used inside stored routing data.

use collaboration_central_core::ObjectAddress;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[test]
fn copy_address_round_trip_preserves_the_original_persisted_encoding()
-> Result<(), Box<dyn std::error::Error>> {
    // Golden encoding from the ProjectId-backed address at 3ca2acea.
    let original = br#"{"kind":"copy","value":"019f0000-0000-7000-8000-000000000001"}"#;
    let decoded: ObjectAddress = serde_json::from_slice(original)?;
    assert!(matches!(decoded, ObjectAddress::Copy(_)));
    let encoded = serde_json::to_vec(&decoded)?;
    assert_eq!(encoded, original);
    assert_eq!(serde_json::from_slice::<ObjectAddress>(&encoded)?, decoded);
    Ok(())
}
