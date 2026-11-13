use sqlx_core::row::Row;
use sqlx_core::type_info::TypeInfo;
use sqlx_core::value::ValueRef;
use sqlx_durable_object_core::Value;

use crate::DoRow;
use crate::DoTypeInfo;

#[test]
fn a_column_is_typed_by_its_first_non_null_value() {
    let rows = DoRow::from_result(
        vec!["id".into(), "name".into()],
        vec![
            vec![Value::Integer(1), Value::Null],
            vec![Value::Integer(2), Value::Text("b".into())],
        ],
    )
    .unwrap();

    let types: Vec<_> = rows[0]
        .columns()
        .iter()
        .map(|column| column.type_info)
        .collect();
    assert_eq!(types, [DoTypeInfo::Integer, DoTypeInfo::Text]);
}

#[test]
fn a_name_finds_the_last_column_that_has_it() {
    let rows = DoRow::from_result(
        vec!["id".into(), "id".into()],
        vec![vec![Value::Integer(1), Value::Integer(2)]],
    )
    .unwrap();

    let row = &rows[0];
    assert_eq!(row.try_get::<i64, _>(0).unwrap(), 1);
    assert_eq!(row.try_get::<i64, _>("id").unwrap(), 2);
    assert!(row.try_get_raw("missing").is_err());
}

#[test]
fn a_value_carries_its_own_type() {
    let rows = DoRow::from_result(
        vec!["n".into()],
        vec![vec![Value::Integer(1)], vec![Value::Real(1.5)]],
    )
    .unwrap();

    // The column says INTEGER, from the first row; the second row's value
    // is still a REAL, and that is what decoding checks.
    assert_eq!(rows[1].try_get_raw(0).unwrap().type_info().name(), "REAL");
    assert!(rows[1].try_get::<i64, _>(0).is_err());
    assert!((rows[1].try_get::<f64, _>(0).unwrap() - 1.5).abs() < f64::EPSILON);
}
