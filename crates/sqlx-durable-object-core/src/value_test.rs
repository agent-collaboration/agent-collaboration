use super::{MAX_SAFE_INTEGER, Value, safe_integer};

#[test]
fn whole_numbers_in_the_safe_range_are_integers() {
    assert_eq!(Value::from_number(3.0), Value::Integer(3));
    assert_eq!(Value::from_number(-0.0), Value::Integer(0));
    assert_eq!(
        Value::from_number(9_007_199_254_740_991.0),
        Value::Integer(MAX_SAFE_INTEGER)
    );
}

#[test]
fn everything_else_is_real() {
    assert_eq!(Value::from_number(1.5), Value::Real(1.5));
    // 2^53: whole, but no longer exact -- 2^53 + 1 would read the same.
    assert_eq!(
        Value::from_number(9_007_199_254_740_992.0),
        Value::Real(9_007_199_254_740_992.0)
    );
    assert!(matches!(Value::from_number(f64::INFINITY), Value::Real(_)));
    assert!(matches!(Value::from_number(f64::NAN), Value::Real(_)));
}

#[test]
fn safe_integer_accepts_exactly_the_safe_range() {
    assert!(safe_integer(MAX_SAFE_INTEGER).is_ok());
    assert!(safe_integer(-MAX_SAFE_INTEGER).is_ok());
    assert!(safe_integer(MAX_SAFE_INTEGER + 1).is_err());
    assert!(safe_integer(-MAX_SAFE_INTEGER - 1).is_err());
    assert!(safe_integer(i64::MIN).is_err());
}

#[test]
fn accessors_accept_what_their_types_are_compatible_with() {
    assert_eq!(Value::Integer(2).int().unwrap(), 2);
    assert!(Value::Real(2.0).int().is_err());
    assert!((Value::Integer(2).real().unwrap() - 2.0).abs() < f64::EPSILON);
    assert_eq!(Value::Text("ab".into()).blob().unwrap(), b"ab");
    assert!(Value::Blob(b"ab".to_vec()).text().is_err());
    assert!(Value::Null.int().is_err());
}
