use super::{RowExpectation, SafeInteger, SqlValue, UnsafeInteger};

#[test]
fn row_expectations_admit_only_their_counts() {
    assert!(RowExpectation::Any.admits(0));
    assert!(RowExpectation::Any.admits(7));
    assert!(RowExpectation::Exactly(1).admits(1));
    assert!(!RowExpectation::Exactly(1).admits(0));
    assert!(!RowExpectation::Exactly(1).admits(2));
    assert!(RowExpectation::AtLeastOne.admits(3));
    assert!(!RowExpectation::AtLeastOne.admits(0));
}

#[test]
fn integers_beyond_two_to_the_53_minus_one_are_refused_at_construction() {
    assert_eq!(
        SafeInteger::new(9_007_199_254_740_991).map(SafeInteger::get),
        Ok(9_007_199_254_740_991)
    );
    assert_eq!(
        SafeInteger::new(-9_007_199_254_740_991).map(SafeInteger::get),
        Ok(-9_007_199_254_740_991)
    );
    assert_eq!(
        SafeInteger::new(9_007_199_254_740_992),
        Err(UnsafeInteger {
            value: 9_007_199_254_740_992
        })
    );
    assert_eq!(
        SafeInteger::new(-9_007_199_254_740_992),
        Err(UnsafeInteger {
            value: -9_007_199_254_740_992
        })
    );
}

#[test]
fn the_wire_form_refuses_an_unsafe_integer() {
    let safe: SqlValue =
        serde_json::from_str(r#"{"kind":"integer","value":9007199254740991}"#).unwrap();
    assert_eq!(safe, SqlValue::Integer(SafeInteger::MAX));
    assert!(
        serde_json::from_str::<SqlValue>(r#"{"kind":"integer","value":9007199254740992}"#).is_err()
    );
    assert!(serde_json::from_str::<SqlValue>(r#"{"kind":"real","value":1.5}"#).is_err());
}
