use serde_json::{Number, Value};

fn integer(number: &Number) -> Option<i128> {
    number
        .as_i64()
        .map(i128::from)
        .or_else(|| number.as_u64().map(i128::from))
}
fn equal_numbers(left: &Number, right: &Number) -> bool {
    match (integer(left), integer(right)) {
        (Some(left), Some(right)) => left == right,
        (Some(integer), None) => right
            .as_f64()
            .is_some_and(|value| value.fract() == 0.0 && value as i128 == integer),
        (None, Some(integer)) => left
            .as_f64()
            .is_some_and(|value| value.fract() == 0.0 && value as i128 == integer),
        (None, None) => left.as_f64() == right.as_f64(),
    }
}
#[must_use]
pub fn same_json(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Bool(left), Value::Number(right)) => {
            equal_numbers(&Number::from(u8::from(*left)), right)
        }
        (Value::Number(left), Value::Bool(right)) => {
            equal_numbers(left, &Number::from(u8::from(*right)))
        }
        (Value::Number(left), Value::Number(right)) => equal_numbers(left, right),
        (Value::Array(left), Value::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| same_json(left, right))
        }
        (Value::Object(left), Value::Object(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .all(|(key, value)| right.get(key).is_some_and(|other| same_json(value, other)))
        }
        _ => left == right,
    }
}
