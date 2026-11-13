use super::rows;
use crate::{TypeInfo, Value};

fn types(names: &[&str], values: Vec<Vec<Value>>) -> Vec<TypeInfo> {
    let names = names.iter().map(|name| (*name).to_owned()).collect();
    let rows = rows(
        names,
        values,
        |_, _, type_info| type_info,
        |columns, _| columns,
    )
    .unwrap();
    rows[0].to_vec()
}

#[test]
fn a_column_is_typed_by_its_first_non_null_value() {
    let types = types(
        &["id", "name", "gone"],
        vec![
            vec![Value::Integer(1), Value::Null, Value::Null],
            vec![Value::Integer(2), Value::Text("b".into()), Value::Null],
        ],
    );

    assert_eq!(types, [TypeInfo::Integer, TypeInfo::Text, TypeInfo::Null]);
}

#[test]
fn a_short_row_is_an_error() {
    assert!(
        rows(
            vec!["a".into(), "b".into()],
            vec![vec![Value::Null]],
            |_, _, _| (),
            |_, _| ()
        )
        .is_err()
    );
}
