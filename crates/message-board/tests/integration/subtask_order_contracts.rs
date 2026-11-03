use message_board::{SubtaskPlace, SubtaskPlaceError};
use proptest::prelude::*;

#[test]
fn supplied_and_generated_order_keys_obey_the_eight_kibibyte_hex_limit() {
    let maximum = format!("{}01", "00".repeat(4095));
    let upper = SubtaskPlace::try_from(maximum.clone()).unwrap();
    assert_eq!(String::from(upper.clone()), maximum);
    let too_long = format!("{}01", "00".repeat(4096));
    let expected = SubtaskPlaceError::OrderKeyTooLong {
        length: 8194,
        limit: 8192,
    };
    assert_eq!(
        SubtaskPlace::try_from(too_long.clone()),
        Err(expected.clone())
    );
    assert!(serde_json::from_value::<SubtaskPlace>(serde_json::json!(too_long)).is_err());
    assert_eq!(SubtaskPlace::between(None, Some(&upper)), Err(expected));
    assert_eq!(
        String::from(upper),
        maximum,
        "generation must not rebalance its bounds"
    );
}

#[test]
fn keys_validate_at_decode_and_reject_reversed_or_equal_bounds() {
    for invalid in ["", "0", "FF", "gg", "00", "1200"] {
        assert!(
            SubtaskPlace::try_from(invalid.to_owned()).is_err(),
            "{invalid:?}"
        );
        assert!(serde_json::from_value::<SubtaskPlace>(serde_json::json!(invalid)).is_err());
    }
    let low = SubtaskPlace::try_from("ff".to_owned()).unwrap();
    let high = SubtaskPlace::try_from("ff01".to_owned()).unwrap();
    assert!(SubtaskPlace::between(Some(&high), Some(&low)).is_err());
    assert!(SubtaskPlace::between(Some(&low), Some(&low)).is_err());
    let middle = SubtaskPlace::between(Some(&low), Some(&high)).unwrap();
    assert!(low < middle && middle < high);
}

proptest! {
    #[test]
    fn repeated_insertions_preserve_bounds_and_serialized_order(
        slots in prop::collection::vec(any::<usize>(), 0..200)
    ) {
        let mut checklist: Vec<SubtaskPlace> = Vec::new();
        for slot in slots {
            let index = slot % (checklist.len() + 1);
            let lower = index.checked_sub(1).map(|lower| &checklist[lower]);
            let upper = checklist.get(index);
            let inserted = SubtaskPlace::between(lower, upper).unwrap();
            prop_assert!(lower.is_none_or(|lower| lower < &inserted));
            prop_assert!(upper.is_none_or(|upper| &inserted < upper));
            let encoded = serde_json::to_value(&inserted).unwrap();
            prop_assert_eq!(serde_json::from_value::<SubtaskPlace>(encoded).unwrap(), inserted.clone());
            checklist.insert(index, inserted);
            let wire: Vec<String> = checklist.iter().cloned().map(String::from).collect();
            prop_assert!(wire.windows(2).all(|neighbors| neighbors[0] < neighbors[1]));
        }
    }
}
