#![allow(clippy::expect_used)]
use collaboration_central_core::{BoundValue, SqlRow, StoreError};
#[test]
fn integer_text_is_decoded_without_a_double_and_real_kind_is_retained() {
    let row = SqlRow::from_transport(vec![(
        "record_value".into(),
        BoundValue::IntegerText("9007199254740993".into()),
    )])
    .expect("row");
    assert_eq!(row.integer("record_value").expect("i64"), 9007199254740993);
    let real = SqlRow::from_transport(vec![("record_value".into(), BoundValue::Real(1.0))])
        .expect("real row");
    assert_eq!(
        real.value("record_value").expect("real"),
        &collaboration_replication_protocol::SqlValue::Real(1.0)
    );
}
#[test]
fn invalid_integer_text_is_a_stored_integrity_error() {
    let row = SqlRow::from_transport(vec![(
        "record_value".into(),
        BoundValue::IntegerText("9223372036854775808".into()),
    )]);
    assert!(matches!(row, Err(StoreError::Corrupt { .. })));
}
