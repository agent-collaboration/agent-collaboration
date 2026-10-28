use serde_json::{Number, Value};

fn integer(number: &Number) -> Option<i128> {
    number
        .as_i64()
        .map(i128::from)
        .or_else(|| number.as_u64().map(i128::from))
}
fn equal_float_integer(number: &Number, integer: i128) -> bool {
    number
        .as_f64()
        .filter(|value| value.fract() == 0.0)
        .and_then(|value| format!("{value:.0}").parse::<i128>().ok())
        == Some(integer)
}
fn equal_numbers(left: &Number, right: &Number) -> bool {
    match (integer(left), integer(right)) {
        (Some(left), Some(right)) => left == right,
        (Some(integer), None) => equal_float_integer(right, integer),
        (None, Some(integer)) => equal_float_integer(left, integer),
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

#[cfg(test)]
mod json_equality_tests {
    use super::same_json;
    use serde_json::Value;

    #[test]
    fn mixed_number_equality_preserves_integer_precision() -> Result<(), Box<dyn std::error::Error>>
    {
        // Arrange: integral floats compare by their exact value, including at integer limits.
        let cases = [
            ("1", "1.0", true),
            ("0", "-0.0", true),
            ("1", "1.25", false),
            ("9007199254740993", "9007199254740992.0", false),
            ("9223372036854775807", "9223372036854775808.0", false),
            ("18446744073709551615", "18446744073709551616.0", false),
            ("18446744073709549568", "18446744073709549568.0", true),
            ("-9223372036854775808", "-9223372036854775808.0", true),
            ("1", "1e100", false),
        ];
        for (integer, floating, expected) in cases {
            let integer: Value = serde_json::from_str(integer)?;
            let floating: Value = serde_json::from_str(floating)?;

            // Act and assert: equality is symmetric and does not round a stored integer.
            if same_json(&integer, &floating) != expected
                || same_json(&floating, &integer) != expected
            {
                return Err(format!(
                    "mixed-number equality for {integer} and {floating} should be {expected}"
                )
                .into());
            }
        }
        Ok(())
    }
}
